// SPDX-License-Identifier: MPL-2.0

//! FreeBSD-family hosts: stock FreeBSD and Renux.

pub mod ptrace;
pub mod renux;
pub mod setscret;

use std::io;
use std::os::fd::{FromRawFd, OwnedFd};

use crate::caps::{Capabilities, RenuxExtensions, SandboxKind, TrapBackendKind};
use crate::process::{Child, ForkResult};
use crate::sys;

/// `mmap` flags: place exactly at the address, fail if it is in use.
pub(crate) const MAP_FIXED_NOREPLACE: libc::c_int = libc::MAP_FIXED | libc::MAP_EXCL;

/// `errno` from `mmap` with `MAP_EXCL` when the range is already mapped.
pub(crate) const MAP_IN_USE_ERRNO: libc::c_int = libc::ENOMEM;

pub(crate) fn
shm_create_anonymous() -> io::Result<OwnedFd>
{
    // SAFETY: SHM_ANON creates an unnamed object; no pointers are retained.
    let fd: libc::c_int = sys::check(unsafe
    {
        libc::shm_open(libc::SHM_ANON, libc::O_RDWR | libc::O_CLOEXEC, 0o600)
    })?;

    // SAFETY: `fd` was just returned by shm_open and is owned by nobody else.
    let owned: OwnedFd = unsafe { OwnedFd::from_raw_fd(fd) };

    return (Ok(owned));
}

/// Forks with `pdfork(2)`: the child is killed when its descriptor closes.
pub(crate) unsafe fn
fork() -> io::Result<ForkResult>
{
    let mut fd: libc::c_int = -1;

    // SAFETY: `fd` is a valid output location; the caller upholds the
    // post-fork async-signal-safety contract.
    let pid: libc::pid_t = unsafe { libc::pdfork(&mut fd, libc::PD_CLOEXEC) };

    if pid == -1
    {
        return (Err(io::Error::last_os_error()));
    }
    if pid == 0
    {
        return (Ok(ForkResult::Child));
    }

    // SAFETY: pdfork returned a fresh process descriptor in `fd`.
    let handle: OwnedFd = unsafe { OwnedFd::from_raw_fd(fd) };

    return (Ok(ForkResult::Parent(Child::new(pid, Some(handle)))));
}

/// Kills a child, including one stopped under ptrace: FreeBSD leaves a
/// `SIGKILL` pending on a traced, stopped process until the tracer resumes
/// it, so a traced child is killed with `PT_KILL` instead.
pub(crate) fn
terminate(pid: libc::pid_t) -> ()
{
    if ptrace::kill(pid).is_err()
    {
        // SAFETY: plain system call on a pid the caller owns.
        unsafe
        {
            libc::kill(pid, libc::SIGKILL);
        }
    }
}

pub(crate) fn
enter_sandbox() -> io::Result<()>
{
    // SAFETY: plain system call without arguments.
    sys::check(unsafe { libc::cap_enter() })?;

    return (Ok(()));
}

pub(crate) fn
in_sandbox() -> io::Result<bool>
{
    let mut mode: libc::c_uint = 0;

    // SAFETY: `mode` is a valid output location.
    sys::check(unsafe { libc::cap_getmode(&mut mode) })?;

    return (Ok(mode != 0));
}

pub(crate) fn
probe() -> io::Result<Capabilities>
{
    let uname: sys::Uname = sys::uname()?;
    let renux: RenuxExtensions = RenuxExtensions
    {
        // A failed probe (for example ptrace forbidden inside a jail) means
        // the extension cannot be used, which is all that matters here.
        set_sc_ret: renux::probe_set_sc_ret().unwrap_or(false),
    };
    let mut trap_backends: Vec<TrapBackendKind> = Vec::new();

    if renux.set_sc_ret
    {
        trap_backends.push(TrapBackendKind::RenuxSetScRet);
    }
    trap_backends.push(TrapBackendKind::FreeBsdRewrite);

    let caps: Capabilities = Capabilities
    {
        os: uname.os,
        release: uname.release,
        machine: uname.machine,
        renux: renux,
        trap_backends: trap_backends,
        sandbox: SandboxKind::Capsicum,
    };

    return (Ok(caps));
}

/// Reads up to `buf.len()` random bytes with `getrandom(2)`.
pub(crate) fn
random_chunk(buf: &mut [u8]) -> io::Result<usize>
{
    // SAFETY: `buf` is valid for writes of `buf.len()` bytes.
    let ret: libc::ssize_t =
        unsafe { libc::getrandom(buf.as_mut_ptr().cast::<libc::c_void>(), buf.len(), 0) };

    if ret < 0
    {
        return (Err(io::Error::last_os_error()));
    }

    return (Ok(ret as usize));
}

/// Renux uses `renux-setscret`. The stock FreeBSD backend
/// (`freebsd-rewrite`) is not implemented yet.
pub(crate) fn
open_trap_backend(caps: &crate::Capabilities) -> io::Result<Box<dyn crate::trap::TrapBackend>>
{
    if caps.renux.set_sc_ret
    {
        return (Ok(Box::new(setscret::RenuxSetScRet::new())));
    }

    return (Err(io::Error::new(io::ErrorKind::Unsupported, "freebsd-rewrite is not implemented yet")));
}

/// Host system call numbers the guest stub executes.
pub(crate) const SYS_MMAP: u64 = 477;
pub(crate) const SYS_MUNMAP: u64 = 73;
pub(crate) const SYS_MPROTECT: u64 = 74;

/// End of the user address space of an LA48 process
/// (`VM_MAXUSER_ADDRESS_LA48`); the shared page is just below it.
pub(crate) const USER_TOP: u64 = 0x8000_0000_0000;

/// Closes every descriptor from `fd` up. Async-signal-safe.
pub(crate) fn
close_from(fd: libc::c_int) -> ()
{
    // SAFETY: closefrom has no memory-safety preconditions.
    unsafe
    {
        libc::closefrom(fd);
    }
}
