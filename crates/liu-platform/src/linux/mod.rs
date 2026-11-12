// SPDX-License-Identifier: MPL-2.0

//! Linux hosts (glibc and musl).

use std::ffi::CStr;
use std::io;
use std::os::fd::{FromRawFd, OwnedFd};

use crate::caps::{Capabilities, RenuxExtensions, SandboxKind, TrapBackendKind};
use crate::process::{Child, ForkResult};
use crate::sys;

const MFD_CLOEXEC: libc::c_uint = 0x0001;

/// `mmap` flags: place exactly at the address, fail if it is in use. Kernels
/// before 4.17 ignore the flag and treat the address as a hint, which
/// `vm::map_exact` detects.
pub(crate) const MAP_FIXED_NOREPLACE: libc::c_int = libc::MAP_FIXED_NOREPLACE;

/// `errno` from `mmap` with `MAP_FIXED_NOREPLACE` when the range is in use.
pub(crate) const MAP_IN_USE_ERRNO: libc::c_int = libc::EEXIST;

pub(crate) fn
shm_create_anonymous() -> io::Result<OwnedFd>
{
    let name: &CStr = c"liu-physmem";

    // memfd_create through syscall(2): the libc wrapper is missing on some
    // musl versions.
    // SAFETY: `name` is NUL-terminated and outlives the call.
    let ret: libc::c_long = unsafe
    {
        libc::syscall(libc::SYS_memfd_create, name.as_ptr(), MFD_CLOEXEC)
    };

    if ret < 0
    {
        return (Err(io::Error::last_os_error()));
    }

    // SAFETY: the kernel returned a fresh descriptor owned by nobody else.
    let owned: OwnedFd = unsafe { OwnedFd::from_raw_fd(ret as libc::c_int) };

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
    // Landlock + seccomp: planned (architecture.md §12).
    return (Err(io::Error::from(io::ErrorKind::Unsupported)));
}

pub(crate) fn
in_sandbox() -> io::Result<bool>
{
    return (Ok(false));
}

fn
kvm_available() -> bool
{
    let path: &CStr = c"/dev/kvm";

    // SAFETY: `path` is NUL-terminated.
    let ret: libc::c_int = unsafe { libc::access(path.as_ptr(), libc::R_OK | libc::W_OK) };

    return (ret == 0);
}

pub(crate) fn
probe() -> io::Result<Capabilities>
{
    let uname: sys::Uname = sys::uname()?;
    let mut trap_backends: Vec<TrapBackendKind> = Vec::new();

    if kvm_available()
    {
        trap_backends.push(TrapBackendKind::Kvm);
    }
    trap_backends.push(TrapBackendKind::LinuxSysemu);

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

/// `linux-sysemu` is not implemented yet.
pub(crate) fn
open_trap_backend(_caps: &crate::Capabilities) -> io::Result<Box<dyn crate::trap::TrapBackend>>
{
    return (Err(io::Error::new(io::ErrorKind::Unsupported, "linux-sysemu is not implemented yet")));
}

/// Host system call numbers the guest stub executes.
pub(crate) const SYS_MMAP: u64 = 9;
pub(crate) const SYS_MUNMAP: u64 = 11;
pub(crate) const SYS_MPROTECT: u64 = 10;

/// End of the user address space with 4-level paging (`TASK_SIZE_MAX`).
pub(crate) const USER_TOP: u64 = 0x7fff_ffff_f000;

/// Closes every descriptor from `fd` up. Async-signal-safe.
// Used by the guest space setup of this host's trap backend, which is
// not implemented yet.
#[allow(dead_code)]
pub(crate) fn
close_from(fd: libc::c_int) -> ()
{
    // SAFETY: close_range has no memory-safety preconditions; it is called
    // through syscall(2) because older libcs lack the wrapper.
    unsafe
    {
        libc::syscall(libc::SYS_close_range, fd as libc::c_uint, libc::c_uint::MAX, 0);
    }
}
