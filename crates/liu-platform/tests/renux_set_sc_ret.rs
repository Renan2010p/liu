// SPDX-License-Identifier: MPL-2.0

//! The Renux `PT_SET_SC_RET` primitive, from Rust.
//!
//! This is the sequence the `renux-setscret` trap backend relies on. The
//! tests skip on a FreeBSD kernel without the extension.

#![cfg(target_os = "freebsd")]

use std::io;

use liu_platform::freebsd::ptrace::{self, ScResult};
use liu_platform::freebsd::renux;
use liu_platform::{Child, ForkResult, WaitStatus};

/// `getpid(2)` system call number on FreeBSD (sys/syscall.h).
const SYS_GETPID: libc::c_uint = 20;

/// Child: stop, call getpid, exit 42 if it returned `expected` or failed
/// with `EPERM` when `expected` is -1.
fn
tracee(expected: libc::pid_t) -> !
{
    if ptrace::trace_me().is_err()
    {
        // SAFETY: _exit is async-signal-safe.
        unsafe { libc::_exit(90) };
    }

    // SAFETY: raise and getpid are async-signal-safe; errno is read right
    // after the call that set it.
    let (pid, errno): (libc::pid_t, libc::c_int) = unsafe
    {
        libc::raise(libc::SIGSTOP);
        let pid: libc::pid_t = libc::getpid();
        (pid, *libc::__error())
    };
    let ok: bool = if expected == -1
    {
        pid == -1 && errno == libc::EPERM
    }
    else
    {
        pid == expected
    };

    // SAFETY: _exit is async-signal-safe.
    unsafe { libc::_exit(if ok { 42 } else { 1 }) };
}

/// Starts a tracee and leaves it stopped at the `getpid` entry.
fn
start(expected: libc::pid_t) -> Child
{
    // SAFETY: the child branch only runs async-signal-safe code.
    let fork: ForkResult = unsafe { liu_platform::fork().unwrap() };
    let mut child: Child = match fork
    {
        ForkResult::Child =>
        {
            tracee(expected);
        }
        ForkResult::Parent(child) => child,
    };

    assert!(matches!(child.wait().unwrap(), WaitStatus::Stopped(_)));
    ptrace::to_sce(child.pid()).unwrap();
    assert!(matches!(child.wait().unwrap(), WaitStatus::Stopped(_)));

    let info: libc::ptrace_lwpinfo = ptrace::lwpinfo(child.pid()).unwrap();

    assert!(ptrace::at_syscall_entry(&info));
    assert_eq!(info.pl_syscall_code, SYS_GETPID);

    return (child);
}

fn
supported() -> bool
{
    let present: bool = renux::probe_set_sc_ret().unwrap_or(false);

    if !present
    {
        println!("skip: kernel without PT_SET_SC_RET (stock FreeBSD)");
    }

    return (present);
}

#[test]
fn
injects_a_result_without_running_the_call() -> ()
{
    if !supported()
    {
        return;
    }

    let mut child: Child = start(0x1234);

    ptrace::set_sc_ret(child.pid(), ScResult::Value(0x1234)).unwrap();
    assert_eq!(ptrace::run_to_exit(&mut child).unwrap(), WaitStatus::Exited(42));
}

#[test]
fn
injects_an_error_without_running_the_call() -> ()
{
    if !supported()
    {
        return;
    }

    let mut child: Child = start(-1);

    ptrace::set_sc_ret(child.pid(), ScResult::Error(libc::EPERM)).unwrap();
    assert_eq!(ptrace::run_to_exit(&mut child).unwrap(), WaitStatus::Exited(42));
}

#[test]
fn
is_rejected_outside_a_syscall_entry() -> ()
{
    if !supported()
    {
        return;
    }

    let mut child: Child = start(0x1234);

    ptrace::to_scx(child.pid()).unwrap();
    assert!(matches!(child.wait().unwrap(), WaitStatus::Stopped(_)));

    let error: io::Error = ptrace::set_sc_ret(child.pid(), ScResult::Value(1)).unwrap_err();

    assert_eq!(error.raw_os_error(), Some(libc::EINVAL));
    ptrace::kill(child.pid()).unwrap();
    assert!(child.wait().unwrap().is_terminated());
}
