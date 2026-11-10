// SPDX-License-Identifier: MPL-2.0

//! Process creation, probing and sandboxing on the running host.

use liu_platform::{Capabilities, Child, ForkResult, HostOs, WaitStatus};

/// Forks a child that runs `body` and exits with its result.
fn
spawn(body: fn() -> i32) -> Child
{
    // SAFETY: every body passed here is async-signal-safe.
    let fork: ForkResult = unsafe { liu_platform::fork().unwrap() };

    let child: Child = match fork
    {
        ForkResult::Child =>
        {
            let status: i32 = body();

            // SAFETY: _exit is async-signal-safe.
            unsafe { libc::_exit(status) };
        }
        ForkResult::Parent(child) => child,
    };

    return (child);
}

fn
exit_seven() -> i32
{
    return (7);
}

fn
sleep_forever() -> i32
{
    loop
    {
        // SAFETY: pause is async-signal-safe.
        unsafe { libc::pause() };
    }
}

#[test]
fn
reports_exit_status() -> ()
{
    let mut child: Child = spawn(exit_seven);

    assert_eq!(child.wait().unwrap(), WaitStatus::Exited(7));
}

#[test]
fn
reports_termination_signal() -> ()
{
    let mut child: Child = spawn(sleep_forever);

    child.signal(libc::SIGKILL).unwrap();
    assert_eq!(child.wait().unwrap(), WaitStatus::Signaled(libc::SIGKILL));
}

#[test]
fn
probe_describes_this_host() -> ()
{
    let caps: Capabilities = liu_platform::probe().unwrap();

    println!("host: {} ({}), trap backends: {:?}", caps.host_name(), caps.machine, caps.trap_backends);
    assert!(!caps.release.is_empty());
    if cfg!(target_os = "freebsd")
    {
        assert_eq!(caps.os, HostOs::FreeBsd);
        assert!(caps.best_trap_backend().is_some());
    }
    if cfg!(target_os = "linux")
    {
        assert_eq!(caps.os, HostOs::Linux);
    }
}

#[cfg(target_os = "freebsd")]
fn
sandboxed_open_fails() -> i32
{
    if liu_platform::enter_sandbox().is_err()
    {
        return (1);
    }
    if !matches!(liu_platform::in_sandbox(), Ok(true))
    {
        return (2);
    }

    // SAFETY: the path is a NUL-terminated literal.
    let fd: libc::c_int = unsafe { libc::open(c"/etc/passwd".as_ptr(), libc::O_RDONLY) };

    if fd != -1
    {
        return (3);
    }

    return (0);
}

#[cfg(target_os = "freebsd")]
#[test]
fn
sandbox_forbids_opening_paths() -> ()
{
    assert!(!liu_platform::in_sandbox().unwrap());

    let mut child: Child = spawn(sandboxed_open_fails);

    assert_eq!(child.wait().unwrap(), WaitStatus::Exited(0));
}
