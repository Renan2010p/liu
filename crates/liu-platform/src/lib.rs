// SPDX-License-Identifier: MPL-2.0

//! LIU platform layer.
//!
//! This crate is the only place in LIU that knows which host it runs on.
//! Every other crate uses the host-independent API exported here and must not
//! use `#[cfg(target_os = ...)]` (see ADR 0006).
//!
//! Host support:
//!
//! - `freebsd` — Renux and stock FreeBSD. Renux is detected at runtime by
//!   probing its kernel extensions, so one build serves both.
//! - `linux` — Linux with glibc or musl.
//! - `posix` — every other Unix (NetBSD, DragonFly, macOS, OpenBSD,
//!   illumos), using only POSIX interfaces.

#![deny(unsafe_op_in_unsafe_fn)]

mod caps;
mod process;
mod shm;
mod sys;
mod time;
mod trap;
mod vm;

#[cfg(target_os = "freebsd")]
pub mod freebsd;
#[cfg(target_os = "freebsd")]
use freebsd as host;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "linux")]
use linux as host;

#[cfg(not(any(target_os = "freebsd", target_os = "linux")))]
mod generic;
#[cfg(not(any(target_os = "freebsd", target_os = "linux")))]
use generic as host;

use std::io;

pub use caps::{Capabilities, HostOs, RenuxExtensions, SandboxKind, TrapBackendKind};
pub use process::{Child, ForkResult, WaitStatus};
pub use shm::{Mapping, Protection, SharedMemory};
pub use time::{clock_monotonic_ns, clock_realtime_ns, fill_random};
pub use trap::{open_trap_backend, GuestSpace, TrapBackend};
pub use vm::{page_size, Region, Stack, Window};

/// Inspects the running host and reports what LIU can use on it.
///
/// This may fork short-lived helper processes to test kernel features, so
/// call it once at startup, before creating threads if possible.
pub fn
probe() -> io::Result<Capabilities>
{
    return (host::probe());
}

/// Confines the calling process with the strongest sandbox the host offers.
///
/// After this call the process can no longer open paths or sockets; it can
/// only use descriptors it already holds. Fails with `Unsupported` on hosts
/// without a sandbox implementation.
pub fn
enter_sandbox() -> io::Result<()>
{
    return (host::enter_sandbox());
}

/// Returns whether the calling process is sandboxed.
pub fn
in_sandbox() -> io::Result<bool>
{
    return (host::in_sandbox());
}

/// Forks the calling process.
///
/// On hosts with process descriptors the child is bound to the returned
/// [`Child`] and is killed when it is dropped.
///
/// # Safety
///
/// In a multi-threaded process the child may only call async-signal-safe
/// functions until it calls `_exit` or `execve`: no allocation, no locks, no
/// panics.
pub unsafe fn
fork() -> io::Result<ForkResult>
{
    // SAFETY: the caller upholds the async-signal-safety contract.
    let result: io::Result<ForkResult> = unsafe { host::fork() };

    return (result);
}
