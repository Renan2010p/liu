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

use super::ptrace::{self, ScResult, EJUSTRETURN};
use crate::caps::TrapBackendKind;
use crate::process::{Child, ForkResult, WaitStatus};
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

/// Exit status of a space child that was resumed without registers, which
/// the backend never does.
const STRAY_EXIT: i32 = 97;

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

/// Child side of a new space: become traced and stop. Only
/// async-signal-safe calls.
fn
space_child() -> !
{
    if ptrace::trace_me().is_err()
    {
        // SAFETY: _exit is async-signal-safe.
        unsafe { libc::_exit(90) };
    }

    // SAFETY: raise and _exit are async-signal-safe. The tracer replaces
    // the registers before resuming, so `raise` never returns here.
    unsafe
    {
        libc::raise(libc::SIGSTOP);
        libc::_exit(STRAY_EXIT);
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
    create_space(&mut self) -> io::Result<Box<dyn GuestSpace>>
    {
        // SAFETY: the child branch only runs async-signal-safe code.
        let fork: ForkResult = unsafe { crate::fork()? };
        let mut child: Child = match fork
        {
            ForkResult::Child =>
            {
                space_child();
            }
            ForkResult::Parent(child) => child,
        };

        match child.wait()?
        {
            WaitStatus::Stopped(libc::SIGSTOP) =>
            {
            }
            other =>
            {
                return (Err(io::Error::other(format!("new guest space did not stop: {:?}", other))));
            }
        }

        let pid: libc::pid_t = child.pid();
        let host: libc::reg = ptrace::get_regs(pid)?;
        let space: SetScRetSpace = SetScRetSpace
        {
            child: child,
            host: host,
            fs_base: ptrace::get_fs_base(pid)?,
            gs_base: ptrace::get_gs_base(pid)?,
            at_syscall: false,
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
    /// Host register state at the last stop. Segment selectors and other
    /// fields `GuestRegs` does not carry are written back unchanged.
    host: libc::reg,
    fs_base: u64,
    gs_base: u64,
    /// Stopped at a system call entry that must be completed on resume.
    at_syscall: bool,
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

        if self.at_syscall
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
        if self.at_syscall
        {
            ptrace::set_sc_ret(pid, ScResult::Error(EJUSTRETURN))?;
            self.at_syscall = false;
        }

        return (Ok(()));
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
                self.at_syscall = true;
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
