// SPDX-License-Identifier: MPL-2.0

//! Native FreeBSD `ptrace(2)` requests used by the trap backends.
//!
//! These wrappers do not allocate, so the tracee-side calls (`trace_me`) are
//! safe to use in a freshly forked child.

use std::io;
use std::mem::{self, MaybeUninit};
use std::ptr;

use crate::process::{Child, WaitStatus};
use crate::sys;

/// Renux extension: set the result of the system call the tracee is
/// entering and skip its execution. Valid only at a `PT_TO_SCE` stop.
pub const PT_SET_SC_RET: libc::c_int = 45;

/// `PT_FIRSTMACH + 7..10` (`machine/ptrace.h`): the tracee's FS and GS base.
const PT_GETFSBASE: libc::c_int = 71;
const PT_SETFSBASE: libc::c_int = 72;
const PT_GETGSBASE: libc::c_int = 73;
const PT_SETGSBASE: libc::c_int = 74;

/// Kernel-internal errno meaning "leave the registers alone". Passed as the
/// error of [`set_sc_ret`], the skipped call returns with exactly the
/// registers the tracer set, which is how a Linux result (`rax`, nothing
/// else) is delivered without FreeBSD's carry flag and `rdx` conventions.
pub const EJUSTRETURN: i32 = -2;

/// `ptrace(2)` address meaning "resume where the tracee stopped". It is a
/// sentinel, never dereferenced.
fn
resume_in_place() -> *mut libc::c_char
{
    return (ptr::without_provenance_mut::<libc::c_char>(1));
}

/// Result injected with [`set_sc_ret`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScResult
{
    /// The call succeeds and returns this value.
    Value(i64),
    /// The call fails with this host errno.
    Error(i32),
}

fn
request(req: libc::c_int, pid: libc::pid_t, addr: *mut libc::c_char, data: libc::c_int)
    -> io::Result<()>
{
    // SAFETY: every caller passes an `addr` that is valid for `req`.
    sys::check(unsafe { libc::ptrace(req, pid, addr, data) })?;

    return (Ok(()));
}

/// Called in the child: lets the parent trace it.
pub fn
trace_me() -> io::Result<()>
{
    return (request(libc::PT_TRACE_ME, 0, ptr::null_mut(), 0));
}

/// Resumes the tracee until the next system call entry.
pub fn
to_sce(pid: libc::pid_t) -> io::Result<()>
{
    return (request(libc::PT_TO_SCE, pid, resume_in_place(), 0));
}

/// Resumes the tracee until the next system call exit.
pub fn
to_scx(pid: libc::pid_t) -> io::Result<()>
{
    return (request(libc::PT_TO_SCX, pid, resume_in_place(), 0));
}

/// Resumes the tracee, delivering `signo` (0 for none).
pub fn
cont(pid: libc::pid_t, signo: libc::c_int) -> io::Result<()>
{
    return (request(libc::PT_CONTINUE, pid, resume_in_place(), signo));
}

/// Kills the tracee.
pub fn
kill(pid: libc::pid_t) -> io::Result<()>
{
    return (request(libc::PT_KILL, pid, ptr::null_mut(), 0));
}

/// Describes why the tracee stopped.
pub fn
lwpinfo(pid: libc::pid_t) -> io::Result<libc::ptrace_lwpinfo>
{
    let mut info: MaybeUninit<libc::ptrace_lwpinfo> = MaybeUninit::zeroed();
    let size: libc::c_int = mem::size_of::<libc::ptrace_lwpinfo>() as libc::c_int;

    request(libc::PT_LWPINFO, pid, info.as_mut_ptr().cast::<libc::c_char>(), size)?;

    // SAFETY: the kernel filled the structure; it was zeroed beforehand, so
    // any field it did not write is still a valid integer.
    let info: libc::ptrace_lwpinfo = unsafe { info.assume_init() };

    return (Ok(info));
}

/// Returns whether the tracee is stopped at a system call entry.
pub fn
at_syscall_entry(info: &libc::ptrace_lwpinfo) -> bool
{
    return (info.pl_flags & libc::PL_FLAG_SCE != 0);
}

/// Reads the tracee's general-purpose registers.
pub fn
get_regs(pid: libc::pid_t) -> io::Result<libc::reg>
{
    let mut regs: MaybeUninit<libc::reg> = MaybeUninit::zeroed();

    request(libc::PT_GETREGS, pid, regs.as_mut_ptr().cast::<libc::c_char>(), 0)?;

    // SAFETY: the kernel filled the structure.
    let regs: libc::reg = unsafe { regs.assume_init() };

    return (Ok(regs));
}

/// Writes the tracee's general-purpose registers.
pub fn
set_regs(pid: libc::pid_t, regs: &libc::reg) -> io::Result<()>
{
    let addr: *mut libc::c_char = (regs as *const libc::reg).cast_mut().cast::<libc::c_char>();

    return (request(libc::PT_SETREGS, pid, addr, 0));
}

fn
get_base(req: libc::c_int, pid: libc::pid_t) -> io::Result<u64>
{
    let mut base: libc::register_t = 0;

    request(req, pid, (&mut base as *mut libc::register_t).cast::<libc::c_char>(), 0)?;
    return (Ok(base as u64));
}

fn
set_base(req: libc::c_int, pid: libc::pid_t, base: u64) -> io::Result<()>
{
    let mut value: libc::register_t = base as libc::register_t;

    return (request(req, pid, (&mut value as *mut libc::register_t).cast::<libc::c_char>(), 0));
}

/// Reads the tracee's FS base (TLS pointer on x86_64).
pub fn
get_fs_base(pid: libc::pid_t) -> io::Result<u64>
{
    return (get_base(PT_GETFSBASE, pid));
}

/// Sets the tracee's FS base.
pub fn
set_fs_base(pid: libc::pid_t, base: u64) -> io::Result<()>
{
    return (set_base(PT_SETFSBASE, pid, base));
}

/// Reads the tracee's GS base.
pub fn
get_gs_base(pid: libc::pid_t) -> io::Result<u64>
{
    return (get_base(PT_GETGSBASE, pid));
}

/// Sets the tracee's GS base.
pub fn
set_gs_base(pid: libc::pid_t, base: u64) -> io::Result<()>
{
    return (set_base(PT_SETGSBASE, pid, base));
}

/// Renux: completes the system call the tracee is entering with `result`
/// without executing it. Fails with `EINVAL` outside a syscall-entry stop or
/// on a kernel without the extension.
pub fn
set_sc_ret(pid: libc::pid_t, result: ScResult) -> io::Result<()>
{
    let mut psr: libc::ptrace_sc_ret = libc::ptrace_sc_ret { sr_retval: [0, 0], sr_error: 0 };

    match result
    {
        ScResult::Value(value) =>
        {
            psr.sr_retval[0] = value as libc::register_t;
        }
        ScResult::Error(errno) =>
        {
            psr.sr_error = errno;
        }
    }

    let size: libc::c_int = mem::size_of::<libc::ptrace_sc_ret>() as libc::c_int;
    let addr: *mut libc::c_char = (&mut psr as *mut libc::ptrace_sc_ret).cast::<libc::c_char>();

    return (request(PT_SET_SC_RET, pid, addr, size));
}

/// Resumes a stopped tracee with `PT_CONTINUE` until it terminates.
///
/// The tracee may stop again on the way out (for example at the exit of the
/// system call it was stopped in), so every stop is continued.
pub fn
run_to_exit(child: &mut Child) -> io::Result<WaitStatus>
{
    loop
    {
        cont(child.pid(), 0)?;

        let status: WaitStatus = child.wait()?;

        if status.is_terminated()
        {
            return (Ok(status));
        }
    }
}
