// SPDX-License-Identifier: MPL-2.0

//! Child processes.

use std::io;
use std::os::fd::OwnedFd;

use crate::host;
use crate::sys;

/// Outcome of `fork()`.
#[derive(Debug)]
pub enum ForkResult
{
    /// Running in the new child process.
    Child,
    /// Running in the parent; the value describes the child.
    Parent(Child),
}

/// State reported by `wait`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitStatus
{
    /// The child exited with this status.
    Exited(i32),
    /// The child was terminated by this host signal.
    Signaled(i32),
    /// The child is stopped (job control or ptrace) by this host signal.
    Stopped(i32),
}

impl WaitStatus
{
    fn
    from_raw(status: libc::c_int) -> WaitStatus
    {
        if libc::WIFEXITED(status)
        {
            return (WaitStatus::Exited(libc::WEXITSTATUS(status)));
        }
        if libc::WIFSIGNALED(status)
        {
            return (WaitStatus::Signaled(libc::WTERMSIG(status)));
        }

        return (WaitStatus::Stopped(libc::WSTOPSIG(status)));
    }

    /// Returns whether the child no longer exists.
    pub fn
    is_terminated(self) -> bool
    {
        return (matches!(self, WaitStatus::Exited(_) | WaitStatus::Signaled(_)));
    }
}

/// A child process owned by the caller.
///
/// Dropping a `Child` that has not terminated kills and reaps it.
#[derive(Debug)]
pub struct Child
{
    pid: libc::pid_t,
    /// Process descriptor, on hosts that have them.
    handle: Option<OwnedFd>,
    reaped: bool,
}

impl Child
{
    pub(crate) fn
    new(pid: libc::pid_t, handle: Option<OwnedFd>) -> Child
    {
        return (Child { pid: pid, handle: handle, reaped: false });
    }

    /// Host process id.
    pub fn
    pid(&self) -> libc::pid_t
    {
        return (self.pid);
    }

    /// Returns whether the child is bound to a process descriptor.
    pub fn
    has_handle(&self) -> bool
    {
        return (self.handle.is_some());
    }

    /// Waits for the next state change, including ptrace stops.
    pub fn
    wait(&mut self) -> io::Result<WaitStatus>
    {
        let mut raw: libc::c_int = 0;

        loop
        {
            // SAFETY: `raw` is a valid output location.
            let ret: libc::pid_t = unsafe { libc::waitpid(self.pid, &mut raw, 0) };

            if ret == self.pid
            {
                break;
            }

            let error: io::Error = io::Error::last_os_error();

            if error.kind() != io::ErrorKind::Interrupted
            {
                return (Err(error));
            }
        }

        let status: WaitStatus = WaitStatus::from_raw(raw);

        if status.is_terminated()
        {
            self.reaped = true;
        }

        return (Ok(status));
    }

    /// Sends a host signal to the child.
    pub fn
    signal(&self, signo: libc::c_int) -> io::Result<()>
    {
        // SAFETY: plain system call on a pid we own.
        sys::check(unsafe { libc::kill(self.pid, signo) })?;

        return (Ok(()));
    }
}

impl Drop for Child
{
    fn
    drop(&mut self) -> ()
    {
        if self.reaped
        {
            return;
        }

        let mut raw: libc::c_int = 0;

        host::terminate(self.pid);

        // SAFETY: the child is ours and has not been reaped, so the pid still
        // refers to it.
        unsafe
        {
            libc::waitpid(self.pid, &mut raw, 0);
        }
    }
}
