// SPDX-License-Identifier: MPL-2.0

//! Detection of Renux kernel extensions.

use std::io;

use super::ptrace::{self, ScResult};
use crate::process::{Child, ForkResult, WaitStatus};

/// Value the probe injects as the result of `getpid(2)`.
const FAKE_PID: libc::pid_t = 0x1234;

/// Exit status of a probe child that observed the injected value.
const SAW_FAKE: i32 = 42;

/// Child side of a probe: stop, call `getpid`, and report what it returned
/// through the exit status. Only async-signal-safe calls.
fn
probe_child() -> !
{
    if ptrace::trace_me().is_err()
    {
        // SAFETY: _exit is async-signal-safe.
        unsafe { libc::_exit(90) };
    }

    // SAFETY: raise and getpid are async-signal-safe.
    let pid: libc::pid_t = unsafe
    {
        libc::raise(libc::SIGSTOP);
        libc::getpid()
    };
    let status: libc::c_int = if pid == FAKE_PID
    {
        SAW_FAKE
    }
    else
    {
        1
    };

    // SAFETY: _exit is async-signal-safe.
    unsafe { libc::_exit(status) };
}

/// Runs the parent side of the probe on a stopped child.
fn
probe_parent(child: &mut Child) -> io::Result<bool>
{
    let pid: libc::pid_t = child.pid();

    if !matches!(child.wait()?, WaitStatus::Stopped(_))
    {
        return (Err(io::Error::other("probe child did not stop")));
    }

    ptrace::to_sce(pid)?;
    if !matches!(child.wait()?, WaitStatus::Stopped(_))
    {
        return (Err(io::Error::other("probe child did not reach a syscall")));
    }

    let info: libc::ptrace_lwpinfo = ptrace::lwpinfo(pid)?;

    if !ptrace::at_syscall_entry(&info)
    {
        return (Err(io::Error::other("probe child is not at a syscall entry")));
    }

    if let Err(error) = ptrace::set_sc_ret(pid, ScResult::Value(FAKE_PID as i64))
    {
        if error.raw_os_error() == Some(libc::EINVAL)
        {
            return (Ok(false));
        }
        return (Err(error));
    }

    let result: bool = ptrace::run_to_exit(child)? == WaitStatus::Exited(SAW_FAKE);

    return (Ok(result));
}

/// Returns whether the running kernel implements `PT_SET_SC_RET`.
///
/// A child is traced to a `getpid(2)` entry and the request is issued there.
/// The extension is reported only if the child then observes the injected
/// value, so a kernel that accepts the request but ignores it is rejected.
pub fn
probe_set_sc_ret() -> io::Result<bool>
{
    // SAFETY: the child branch only runs async-signal-safe code.
    let fork: ForkResult = unsafe { crate::fork()? };
    let mut child: Child = match fork
    {
        ForkResult::Child =>
        {
            probe_child();
        }
        ForkResult::Parent(child) => child,
    };

    return (probe_parent(&mut child));
}
