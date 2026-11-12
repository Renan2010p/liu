// SPDX-License-Identifier: MPL-2.0

//! Trap backends: running guest user code in a host process and stopping it
//! at every system call, fault and signal (architecture.md §7.2–7.4).
//!
//! A [`GuestSpace`] is one host process that runs Linux user code. Its
//! system calls are never executed by the host: [`GuestSpace::run`] returns
//! a [`TrapEvent`] instead, and the caller (the Linux kernel, through
//! `as_run`) decides the result, writes it into the registers, and runs the
//! space again.
//!
//! The protocol of `run`:
//!
//! - On entry, `regs` is the state the guest resumes with. After a
//!   `TRAP_SYSCALL`, `regs.rax` holds the result in Linux form (a value, or
//!   `-errno`); `rcx` and `r11` are set as the Linux `syscall` instruction
//!   leaves them.
//! - On return, `regs` is the state at the trap. For `TRAP_SYSCALL`,
//!   `code` and `rax` hold the system call number and `rip` points after the
//!   `syscall` instruction. For `TRAP_PAGE_FAULT`, `rip` is the faulting
//!   instruction.
//! - A pending host signal is discarded when the space resumes: guest
//!   signals are delivered by the Linux kernel, by changing the registers.
//!
//! A space contains only the guest stub (see [`crate::stub`]) and the
//! physical memory mapped with [`GuestSpace::map`]. Address-space changes
//! are queued and applied in one batch by [`GuestSpace::flush`], which
//! `run` calls first.

use std::fmt;
use std::io;

use liu_abi::{GuestRegs, TrapEvent};

use crate::caps::{Capabilities, TrapBackendKind};
use crate::host;
use crate::shm::{Protection, SharedMemory};

/// One guest address space: a traced host process.
pub trait GuestSpace: fmt::Debug + Send
{
    /// Runs guest code with `regs` until the next trap (see the module
    /// documentation for the protocol).
    fn
    run(&mut self, regs: &mut GuestRegs) -> io::Result<TrapEvent>;

    /// Queues mapping physical memory `[phys, phys + len)` at guest address
    /// `vaddr`, replacing what was there. Page aligned, below
    /// [`crate::stub::GUEST_TOP`].
    fn
    map(&mut self, vaddr: u64, len: u64, phys: u64, prot: Protection) -> io::Result<()>;

    /// Queues removing the mappings of a guest range.
    fn
    unmap(&mut self, vaddr: u64, len: u64) -> io::Result<()>;

    /// Queues changing the protection of a guest range.
    fn
    protect(&mut self, vaddr: u64, len: u64, prot: Protection) -> io::Result<()>;

    /// Applies the queued changes now.
    fn
    flush(&mut self) -> io::Result<()>;

    /// Host process id of the space, for diagnostics.
    fn
    pid(&self) -> libc::pid_t;
}

/// A way of trapping guest system calls on this host.
pub trait TrapBackend: fmt::Debug + Send
{
    /// Which mechanism this is.
    fn
    kind(&self) -> TrapBackendKind;

    /// Creates a stopped, empty guest space whose memory comes from
    /// `physmem`. The host process keeps no mapping and no descriptor of
    /// the runtime except `physmem`.
    fn
    create_space(&mut self, physmem: &SharedMemory) -> io::Result<Box<dyn GuestSpace>>;
}

/// Opens the fastest trap backend that is implemented on this host.
///
/// Fails with `Unsupported` when none is.
pub fn
open_trap_backend(caps: &Capabilities) -> io::Result<Box<dyn TrapBackend>>
{
    return (host::open_trap_backend(caps));
}
