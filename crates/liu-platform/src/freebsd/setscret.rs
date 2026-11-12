// SPDX-License-Identifier: MPL-2.0

//! `renux-setscret`: the Renux trap backend.
//!
//! The guest runs under `PT_TO_SCE`, so it stops at every system call entry,
//! before the host executes anything. The call is then completed in place:
//!
//! 1. `PT_SETREGS` with the result in `rax` (Linux form) and `rcx`/`r11` as
//!    the Linux `syscall` instruction leaves them;
//! 2. `PT_SET_SC_RET` with the error `EJUSTRETURN`, which makes the Renux
//!    kernel skip the call and return to the guest without touching any
//!    register.
//!
//! One stop per system call, and the guest sees exactly Linux semantics:
//! no carry flag, `rdx` preserved.
//!
//! Faults arrive as `SIGSEGV` stops. FreeBSD does not give the tracer the
//! page-fault error code (`PT_GETREGS` zeroes `r_err` and `r_trapno`), so
//! the access type is derived from the siginfo, see [`fault_access`]. If a
//! kernel ever reports the trap number, its error code is used instead.

use std::io;

use liu_abi::trap::{ACCESS_EXEC, ACCESS_READ, ACCESS_WRITE, TRAP_EXITED, TRAP_PAGE_FAULT};
use liu_abi::trap::{TRAP_SIGNAL, TRAP_SIG_BUS, TRAP_SIG_FPE, TRAP_SIG_ILL, TRAP_SIG_TRAP, TRAP_SYSCALL};
use liu_abi::{GuestRegs, TrapEvent};

use std::os::fd::{AsFd, AsRawFd};

use super::ptrace::{self, ScResult, EJUSTRETURN, PTRACE_SCE};
use crate::caps::TrapBackendKind;
use crate::process::{Child, ForkResult, WaitStatus};
use crate::shm::{Protection, SharedMemory};
use crate::stub::{self, StubOp, StubQueue, GUEST_PHYSMEM_FD, STUB_CODE, STUB_DATA};
use crate::trap::{GuestSpace, TrapBackend};

/// Page-fault error code bits (x86 `tf_err`).
const PF_WRITE: u32 = 0x2;
const PF_INSTRUCTION: u32 = 0x10;

/// x86 page-fault trap number (`T_PAGEFLT`).
const T_PAGEFLT: u32 = 12;

/// `SIGSEGV` codes (`sys/signal.h`).
const SEGV_ACCERR: libc::c_int = 2;

/// Longest x86 instruction: a fault this close after `rip` is a fetch.
const MAX_INSN_LEN: u64 = 15;

/// Exit statuses of a space child that could not set itself up.
const EXIT_TRACE: i32 = 90;
const EXIT_STUB_MAP: i32 = 91;
const EXIT_PHYSMEM_FD: i32 = 92;
const EXIT_UNREGISTER: i32 = 93;

/// User memory that libc and libthr register with the kernel, which the
/// kernel then reads or writes on its own. The stub unmaps that memory, so
/// every registration must be dropped first or the kernel itself faults
/// the guest:
///
/// - `sigfastblock(2)`: a word of the libthr thread, read on signal
///   delivery and return to user mode;
/// - `exterrctl(2)`: libc's extended-error buffer, written whenever a
///   system call returns an error, which `EJUSTRETURN` is.
const SYS_SIGFASTBLOCK: libc::c_int = 573;
const SIGFASTBLOCK_UNSETPTR: libc::c_long = 3;
const SYS_EXTERRCTL: libc::c_int = 592;
const EXTERRCTL_DISABLE: libc::c_long = 2;

/// Drops the kernel registrations listed above. Async-signal-safe. A
/// registration that does not exist (EINVAL, or ENOSYS on older kernels)
/// is not an error.
fn
unregister_user_memory() -> bool
{
    let calls: [(libc::c_int, libc::c_long); 2] =
        [(SYS_SIGFASTBLOCK, SIGFASTBLOCK_UNSETPTR), (SYS_EXTERRCTL, EXTERRCTL_DISABLE)];

    // An unknown system call raises SIGSYS; make it fail with ENOSYS. Guest
    // system calls are never executed, so this changes nothing for them.
    // SAFETY: signal(3) with SIG_IGN is async-signal-safe (sigaction).
    unsafe
    {
        libc::signal(libc::SIGSYS, libc::SIG_IGN);
    }

    for (nr, op) in calls
    {
        // SAFETY: raw system calls whose only pointer argument is null.
        let ret: libc::c_int = unsafe { libc::syscall(nr, op, 0 as libc::c_long, 0 as libc::c_long) };

        // SAFETY: __error returns the calling thread's errno location.
        let errno: libc::c_int = unsafe { *libc::__error() };

        if ret == -1 && errno != libc::EINVAL && errno != libc::ENOSYS
        {
            return (false);
        }
    }

    return (true);
}

/// `rflags` the stub runs with: interrupts enabled, reserved bit 1.
const STUB_RFLAGS: i64 = 0x202;

const PAGE: usize = 4096;

/// The Renux `PT_SET_SC_RET` backend.
#[derive(Debug, Default)]
pub struct RenuxSetScRet
{
}

impl RenuxSetScRet
{
    /// The backend. The caller must have checked that the kernel provides
    /// `PT_SET_SC_RET` (`Capabilities::renux.set_sc_ret`).
    pub fn
    new() -> RenuxSetScRet
    {
        return (RenuxSetScRet {});
    }
}

/// Child side of a new space: become traced, map the stub pages, keep only
/// the physical memory descriptor, and jump into the stub, which unmaps
/// everything else and stops with `int3`. Only async-signal-safe calls,
/// and nothing after the jump uses the stack.
fn
space_child(code_fd: libc::c_int, data_fd: libc::c_int, physmem_fd: libc::c_int) -> !
{
    if ptrace::trace_me().is_err()
    {
        // SAFETY: _exit is async-signal-safe.
        unsafe { libc::_exit(EXIT_TRACE) };
    }

    let fixed: libc::c_int = libc::MAP_SHARED | libc::MAP_FIXED | libc::MAP_EXCL;

    // SAFETY: mmap with MAP_EXCL cannot replace an existing mapping; the
    // pages are above anything this process uses.
    let (code, data): (*mut libc::c_void, *mut libc::c_void) = unsafe
    {
        (
            libc::mmap(STUB_CODE as usize as *mut libc::c_void, PAGE, libc::PROT_READ | libc::PROT_EXEC, fixed, code_fd, 0),
            libc::mmap(STUB_DATA as usize as *mut libc::c_void, PAGE, libc::PROT_READ | libc::PROT_WRITE, fixed, data_fd, 0),
        )
    };

    if code == libc::MAP_FAILED || data == libc::MAP_FAILED
    {
        // SAFETY: _exit is async-signal-safe.
        unsafe { libc::_exit(EXIT_STUB_MAP) };
    }

    // SAFETY: dup2 is async-signal-safe; the stub pages stay mapped when
    // their descriptors are closed.
    if physmem_fd != GUEST_PHYSMEM_FD && unsafe { libc::dup2(physmem_fd, GUEST_PHYSMEM_FD) } == -1
    {
        // SAFETY: _exit is async-signal-safe.
        unsafe { libc::_exit(EXIT_PHYSMEM_FD) };
    }
    super::close_from(GUEST_PHYSMEM_FD + 1);

    if !unregister_user_memory()
    {
        // SAFETY: _exit is async-signal-safe.
        unsafe { libc::_exit(EXIT_UNREGISTER) };
    }

    // SAFETY: STUB_CODE holds the stub, which needs no stack and never
    // returns here; the cleanup batch was written before the fork.
    unsafe
    {
        core::arch::asm!("jmp {entry}", entry = in(reg) STUB_CODE, options(noreturn));
    }
}

impl TrapBackend for RenuxSetScRet
{
    fn
    kind(&self) -> TrapBackendKind
    {
        return (TrapBackendKind::RenuxSetScRet);
    }

    fn
    create_space(&mut self, physmem: &SharedMemory) -> io::Result<Box<dyn GuestSpace>>
    {
        let mut queue: StubQueue = StubQueue::new(physmem.size())?;

        queue.cleanup();

        let cleanup: Vec<StubOp> = queue.take();
        let (code_fd, data_fd): (libc::c_int, libc::c_int) = (queue.pages().code_fd(), queue.pages().data_fd());
        let physmem_fd: libc::c_int = physmem.as_fd().as_raw_fd();

        queue.pages().write_batch(&cleanup)?;

        // SAFETY: the child branch only runs async-signal-safe code.
        let fork: ForkResult = unsafe { crate::fork()? };
        let mut child: Child = match fork
        {
            ForkResult::Child =>
            {
                space_child(code_fd, data_fd, physmem_fd);
            }
            ForkResult::Parent(child) => child,
        };

        match child.wait()?
        {
            WaitStatus::Stopped(libc::SIGTRAP) =>
            {
            }
            other =>
            {
                return (Err(io::Error::other(format!("new guest space did not reach the stub: {:?}", other))));
            }
        }
        stub::check_results(&queue.pages().results(cleanup.len())?)?;

        let pid: libc::pid_t = child.pid();
        let space: SetScRetSpace = SetScRetSpace
        {
            host: ptrace::get_regs(pid)?,
            fs_base: ptrace::get_fs_base(pid)?,
            gs_base: ptrace::get_gs_base(pid)?,
            event_mask: ptrace::get_event_mask(pid)?,
            child: child,
            queue: queue,
            syscall_pending: false,
            linux_return: false,
            exited: false,
        };

        return (Ok(Box::new(space)));
    }
}

/// A guest space of the Renux backend.
#[derive(Debug)]
struct SetScRetSpace
{
    child: Child,
    queue: StubQueue,
    /// Host register state at the last stop. Segment selectors and other
    /// fields `GuestRegs` does not carry are written back unchanged.
    host: libc::reg,
    fs_base: u64,
    gs_base: u64,
    /// Event mask without `PTRACE_SCE`, used while the stub runs.
    event_mask: libc::c_int,
    /// Stopped at a host system call entry that must be skipped before the
    /// process runs again.
    syscall_pending: bool,
    /// The guest returns from a system call: `rcx`/`r11` follow Linux.
    linux_return: bool,
    exited: bool,
}

fn
to_guest(host: &libc::reg, fs_base: u64, gs_base: u64) -> GuestRegs
{
    let regs: GuestRegs = GuestRegs
    {
        rax: host.r_rax as u64,
        rbx: host.r_rbx as u64,
        rcx: host.r_rcx as u64,
        rdx: host.r_rdx as u64,
        rsi: host.r_rsi as u64,
        rdi: host.r_rdi as u64,
        rbp: host.r_rbp as u64,
        rsp: host.r_rsp as u64,
        r8: host.r_r8 as u64,
        r9: host.r_r9 as u64,
        r10: host.r_r10 as u64,
        r11: host.r_r11 as u64,
        r12: host.r_r12 as u64,
        r13: host.r_r13 as u64,
        r14: host.r_r14 as u64,
        r15: host.r_r15 as u64,
        rip: host.r_rip as u64,
        rflags: host.r_rflags as u64,
        fs_base: fs_base,
        gs_base: gs_base,
    };

    return (regs);
}

fn
to_host(regs: &GuestRegs, host: &mut libc::reg) -> ()
{
    host.r_rax = regs.rax as i64;
    host.r_rbx = regs.rbx as i64;
    host.r_rcx = regs.rcx as i64;
    host.r_rdx = regs.rdx as i64;
    host.r_rsi = regs.rsi as i64;
    host.r_rdi = regs.rdi as i64;
    host.r_rbp = regs.rbp as i64;
    host.r_rsp = regs.rsp as i64;
    host.r_r8 = regs.r8 as i64;
    host.r_r9 = regs.r9 as i64;
    host.r_r10 = regs.r10 as i64;
    host.r_r11 = regs.r11 as i64;
    host.r_r12 = regs.r12 as i64;
    host.r_r13 = regs.r13 as i64;
    host.r_r14 = regs.r14 as i64;
    host.r_r15 = regs.r15 as i64;
    host.r_rip = regs.rip as i64;
    host.r_rflags = regs.rflags as i64;
}

/// Maps a host signal that is not a page fault to the LIU numbering.
fn
guest_signal(signo: libc::c_int) -> Option<u32>
{
    let mapped: u32 = match signo
    {
        libc::SIGILL => TRAP_SIG_ILL,
        libc::SIGTRAP => TRAP_SIG_TRAP,
        libc::SIGFPE => TRAP_SIG_FPE,
        libc::SIGBUS => TRAP_SIG_BUS,
        _ =>
        {
            return (None);
        }
    };

    return (Some(mapped));
}

/// Access type of a page fault.
///
/// With the x86 error code (`trapno == T_PAGEFLT`) the answer is exact.
/// Without it:
///
/// - a fault inside the instruction at `rip` is an instruction fetch;
/// - `SEGV_ACCERR` (the page is mapped, the access is not allowed) is a
///   write, since every guest mapping that is present is at least readable;
/// - `SEGV_MAPERR` (nothing mapped) is reported as a read. A write then
///   faults a second time with `SEGV_ACCERR` once Linux has mapped the page
///   read-only, which is correct, at the cost of one extra fault.
fn
fault_access(trapno: u32, err: u32, code: libc::c_int, addr: u64, rip: u64) -> u32
{
    if trapno == T_PAGEFLT
    {
        return (error_code_access(err));
    }
    if addr >= rip && addr - rip <= MAX_INSN_LEN
    {
        return (ACCESS_EXEC);
    }
    if code == SEGV_ACCERR
    {
        return (ACCESS_WRITE);
    }

    return (ACCESS_READ);
}

/// Access type from the x86 page-fault error code.
fn
error_code_access(err: u32) -> u32
{
    if err & PF_INSTRUCTION != 0
    {
        return (ACCESS_EXEC);
    }
    if err & PF_WRITE != 0
    {
        return (ACCESS_WRITE);
    }

    return (ACCESS_READ);
}

impl SetScRetSpace
{
    /// Writes `regs` into the stopped process and completes a pending
    /// system call.
    fn
    apply(&mut self, regs: &mut GuestRegs) -> io::Result<()>
    {
        let pid: libc::pid_t = self.child.pid();

        if self.linux_return
        {
            // What the Linux `syscall` instruction leaves behind.
            regs.rcx = regs.rip;
            regs.r11 = regs.rflags;
        }
        to_host(regs, &mut self.host);
        ptrace::set_regs(pid, &self.host)?;
        if regs.fs_base != self.fs_base
        {
            ptrace::set_fs_base(pid, regs.fs_base)?;
            self.fs_base = regs.fs_base;
        }
        if regs.gs_base != self.gs_base
        {
            ptrace::set_gs_base(pid, regs.gs_base)?;
            self.gs_base = regs.gs_base;
        }
        self.skip_pending_syscall()?;
        self.linux_return = false;

        return (Ok(()));
    }

    /// Completes a system call the process is stopped in without executing
    /// it, leaving the registers as set.
    fn
    skip_pending_syscall(&mut self) -> io::Result<()>
    {
        if self.syscall_pending
        {
            ptrace::set_sc_ret(self.child.pid(), ScResult::Error(EJUSTRETURN))?;
            self.syscall_pending = false;
        }

        return (Ok(()));
    }

    /// Runs the queued address-space changes in the stub.
    fn
    run_stub(&mut self) -> io::Result<()>
    {
        if self.queue.is_empty()
        {
            return (Ok(()));
        }

        let ops: Vec<StubOp> = self.queue.take();
        let pid: libc::pid_t = self.child.pid();
        let mut stub_regs: libc::reg = self.host;

        self.queue.pages().write_batch(&ops)?;
        stub_regs.r_rip = STUB_CODE as i64;
        stub_regs.r_rflags = STUB_RFLAGS;
        ptrace::set_regs(pid, &stub_regs)?;
        self.skip_pending_syscall()?;
        // The stub's own system calls must run on the host.
        ptrace::set_event_mask(pid, self.event_mask & !PTRACE_SCE)?;
        ptrace::cont(pid, 0)?;

        match self.child.wait()?
        {
            WaitStatus::Stopped(libc::SIGTRAP) =>
            {
            }
            other =>
            {
                self.exited = !matches!(other, WaitStatus::Stopped(_));
                return (Err(io::Error::other(format!("guest stub did not finish: {:?}", other))));
            }
        }

        return (stub::check_results(&self.queue.pages().results(ops.len())?));
    }

    /// Reads the state of the stopped process into `regs`.
    fn
    capture(&mut self, regs: &mut GuestRegs) -> io::Result<()>
    {
        let pid: libc::pid_t = self.child.pid();

        self.host = ptrace::get_regs(pid)?;
        self.fs_base = ptrace::get_fs_base(pid)?;
        self.gs_base = ptrace::get_gs_base(pid)?;
        *regs = to_guest(&self.host, self.fs_base, self.gs_base);

        return (Ok(()));
    }
}

impl GuestSpace for SetScRetSpace
{
    fn
    run(&mut self, regs: &mut GuestRegs) -> io::Result<TrapEvent>
    {
        if self.exited
        {
            return (Err(io::Error::other("guest space has exited")));
        }
        self.run_stub()?;
        self.apply(regs)?;

        loop
        {
            let pid: libc::pid_t = self.child.pid();

            ptrace::to_sce(pid)?;

            let signo: libc::c_int = match self.child.wait()?
            {
                WaitStatus::Stopped(signo) => signo,
                WaitStatus::Exited(status) =>
                {
                    self.exited = true;
                    return (Ok(TrapEvent { kind: TRAP_EXITED, code: status as u64, ..TrapEvent::default() }));
                }
                WaitStatus::Signaled(signo) =>
                {
                    self.exited = true;
                    return (Ok(TrapEvent { kind: TRAP_EXITED, code: 128 + signo as u64, ..TrapEvent::default() }));
                }
            };
            let info: libc::ptrace_lwpinfo = ptrace::lwpinfo(pid)?;

            if ptrace::at_syscall_entry(&info)
            {
                self.capture(regs)?;
                // The `syscall` instruction loaded rcx and r11 with the
                // return address and flags; FreeBSD's entry code stored r10
                // in the rcx slot, so report what the hardware did.
                regs.rcx = regs.rip;
                regs.r11 = regs.rflags;
                self.syscall_pending = true;
                self.linux_return = true;
                return (Ok(TrapEvent { kind: TRAP_SYSCALL, code: regs.rax, ..TrapEvent::default() }));
            }
            if signo == libc::SIGSEGV
            {
                self.capture(regs)?;

                // SAFETY: a SIGSEGV stop carries fault siginfo, whose address
                // field is initialized by the kernel.
                let addr: u64 = unsafe { info.pl_siginfo.si_addr() } as usize as u64;
                let access: u32 =
                    fault_access(self.host.r_trapno, self.host.r_err, info.pl_siginfo.si_code, addr, regs.rip);
                let event: TrapEvent = TrapEvent
                {
                    kind: TRAP_PAGE_FAULT,
                    access: access,
                    addr: addr,
                    ..TrapEvent::default()
                };

                return (Ok(event));
            }
            if let Some(mapped) = guest_signal(signo)
            {
                self.capture(regs)?;

                // SAFETY: signals raised by a trap carry siginfo whose
                // address field is initialized by the kernel.
                let addr: u64 = unsafe { info.pl_siginfo.si_addr() } as usize as u64;

                return (Ok(TrapEvent { kind: TRAP_SIGNAL, signo: mapped, addr: addr, ..TrapEvent::default() }));
            }
            // A host signal that means nothing to the guest (job control,
            // a stray SIGSTOP): drop it and keep running.
        }
    }

    fn
    map(&mut self, vaddr: u64, len: u64, phys: u64, prot: Protection) -> io::Result<()>
    {
        if self.queue.is_full()
        {
            self.run_stub()?;
        }
        return (self.queue.map(vaddr, len, phys, prot));
    }

    fn
    unmap(&mut self, vaddr: u64, len: u64) -> io::Result<()>
    {
        if self.queue.is_full()
        {
            self.run_stub()?;
        }
        return (self.queue.unmap(vaddr, len));
    }

    fn
    protect(&mut self, vaddr: u64, len: u64, prot: Protection) -> io::Result<()>
    {
        if self.queue.is_full()
        {
            self.run_stub()?;
        }
        return (self.queue.protect(vaddr, len, prot));
    }

    fn
    flush(&mut self) -> io::Result<()>
    {
        if self.exited
        {
            return (Err(io::Error::other("guest space has exited")));
        }
        return (self.run_stub());
    }

    fn
    pid(&self) -> libc::pid_t
    {
        return (self.child.pid());
    }
}

#[cfg(test)]
mod tests
{
    use super::*;

    #[test]
    fn
    decodes_fault_access() -> ()
    {
        let maperr: libc::c_int = 1;

        // Exact, from the error code.
        assert_eq!(fault_access(T_PAGEFLT, 0x4, maperr, 0x1000, 0x5000), ACCESS_READ);
        assert_eq!(fault_access(T_PAGEFLT, 0x6, maperr, 0x1000, 0x5000), ACCESS_WRITE);
        assert_eq!(fault_access(T_PAGEFLT, 0x15, maperr, 0x1000, 0x5000), ACCESS_EXEC);
        // Derived from the siginfo.
        assert_eq!(fault_access(0, 0, SEGV_ACCERR, 0x5003, 0x5000), ACCESS_EXEC);
        assert_eq!(fault_access(0, 0, SEGV_ACCERR, 0x1000, 0x5000), ACCESS_WRITE);
        assert_eq!(fault_access(0, 0, maperr, 0x1000, 0x5000), ACCESS_READ);
    }

    #[test]
    fn
    registers_round_trip() -> ()
    {
        // SAFETY: libc::reg is plain integers; all-zero is valid.
        let mut host: libc::reg = unsafe { std::mem::zeroed() };
        let regs: GuestRegs = GuestRegs { rax: 1, rdx: 2, r15: 3, rip: 4, rflags: 0x202, fs_base: 5, ..GuestRegs::default() };

        to_host(&regs, &mut host);
        assert_eq!(to_guest(&host, 5, 0), regs);
    }
}
