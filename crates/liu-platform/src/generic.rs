// SPDX-License-Identifier: MPL-2.0

//! Every other Unix host (NetBSD, DragonFly, macOS, OpenBSD, illumos),
//! using only POSIX interfaces.

use std::ffi::CString;
use std::io;
use std::os::fd::{FromRawFd, OwnedFd};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::caps::{Capabilities, HostOs, RenuxExtensions, SandboxKind, TrapBackendKind};
use crate::process::{Child, ForkResult};
use crate::sys;

static SHM_COUNTER: AtomicU32 = AtomicU32::new(0);

/// POSIX has no "fixed, no replace" flag: the address is only a hint, and
/// `vm::map_exact` refuses a mapping placed elsewhere.
pub(crate) const MAP_FIXED_NOREPLACE: libc::c_int = 0;

/// Never returned: without the flag, an occupied range shows up as a
/// mapping at another address.
pub(crate) const MAP_IN_USE_ERRNO: libc::c_int = -1;

/// POSIX has no anonymous shared memory object: create a uniquely named one
/// and unlink it immediately, leaving only the descriptor.
pub(crate) fn
shm_create_anonymous() -> io::Result<OwnedFd>
{
    // SAFETY: getpid has no preconditions.
    let pid: libc::pid_t = unsafe { libc::getpid() };
    let serial: u32 = SHM_COUNTER.fetch_add(1, Ordering::Relaxed);
    let name: CString = match CString::new(format!("/liu.{}.{}", pid, serial))
    {
        Ok(name) => name,
        Err(_) =>
        {
            return (Err(io::Error::from(io::ErrorKind::InvalidInput)));
        }
    };
    let flags: libc::c_int = libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC;

    // SAFETY: `name` is NUL-terminated and outlives the calls.
    let fd: libc::c_int = sys::check(unsafe { libc::shm_open(name.as_ptr(), flags, 0o600) })?;

    // SAFETY: as above.
    unsafe
    {
        libc::shm_unlink(name.as_ptr());
    }

    // SAFETY: shm_open returned a fresh descriptor owned by nobody else.
    let owned: OwnedFd = unsafe { OwnedFd::from_raw_fd(fd) };

    return (Ok(owned));
}

pub(crate) unsafe fn
fork() -> io::Result<ForkResult>
{
    // SAFETY: the caller upholds the post-fork async-signal-safety contract.
    let pid: libc::pid_t = unsafe { libc::fork() };

    if pid == -1
    {
        return (Err(io::Error::last_os_error()));
    }
    if pid == 0
    {
        return (Ok(ForkResult::Child));
    }

    return (Ok(ForkResult::Parent(Child::new(pid, None))));
}

/// Kills a child. `SIGKILL` also terminates a ptrace-stopped child here.
pub(crate) fn
terminate(pid: libc::pid_t) -> ()
{
    // SAFETY: plain system call on a pid the caller owns.
    unsafe
    {
        libc::kill(pid, libc::SIGKILL);
    }
}

pub(crate) fn
enter_sandbox() -> io::Result<()>
{
    return (Err(io::Error::from(io::ErrorKind::Unsupported)));
}

pub(crate) fn
in_sandbox() -> io::Result<bool>
{
    return (Ok(false));
}

pub(crate) fn
probe() -> io::Result<Capabilities>
{
    let uname: sys::Uname = sys::uname()?;
    let x86_64: bool = uname.machine == "x86_64" || uname.machine == "amd64";
    let mut trap_backends: Vec<TrapBackendKind> = Vec::new();

    match uname.os
    {
        HostOs::NetBsd =>
        {
            trap_backends.push(TrapBackendKind::NetBsdSyscallEmu);
        }
        HostOs::DragonFly =>
        {
            trap_backends.push(TrapBackendKind::FreeBsdRewrite);
        }
        HostOs::MacOs =>
        {
            if x86_64
            {
                trap_backends.push(TrapBackendKind::Hvf);
            }
        }
        _ =>
        {
        }
    }

    let caps: Capabilities = Capabilities
    {
        os: uname.os,
        release: uname.release,
        machine: uname.machine,
        renux: RenuxExtensions::default(),
        trap_backends: trap_backends,
        sandbox: SandboxKind::None,
    };

    return (Ok(caps));
}

/// Reads up to 256 random bytes with `getentropy(3)`, the portable limit.
pub(crate) fn
random_chunk(buf: &mut [u8]) -> io::Result<usize>
{
    let len: usize = buf.len().min(256);

    // SAFETY: `buf` is valid for writes of `len` bytes.
    sys::check(unsafe { libc::getentropy(buf.as_mut_ptr().cast::<libc::c_void>(), len) })?;

    return (Ok(len));
}

/// No trap backend exists for these hosts yet.
pub(crate) fn
open_trap_backend(_caps: &crate::Capabilities) -> io::Result<Box<dyn crate::trap::TrapBackend>>
{
    return (Err(io::Error::from(io::ErrorKind::Unsupported)));
}
