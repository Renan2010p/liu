// SPDX-License-Identifier: MPL-2.0

//! LIU host ABI.
//!
//! This crate is the single source of truth for the interface between the
//! freestanding Linux kernel image and the LIU runtime. The C header used by
//! the kernel (`kernel/os-liu/include/liu/host.h`) is generated from it with
//! `cargo xtask gen-header` and must never be edited by hand.
//!
//! Everything that crosses the boundary is `#[repr(C)]` and uses only
//! fixed-width integers, raw pointers and nullable function pointers. Rust
//! enums never cross the boundary: values received from the other side are
//! plain integers and are validated before use.

#![no_std]
#![forbid(unsafe_code)]

pub mod boot;
pub mod host_ops;
pub mod kernel_ops;
pub mod layout;
pub mod regs;
pub mod status;
pub mod table;
pub mod trap;

pub use boot::BootInfo;
pub use host_ops::HostOps;
pub use kernel_ops::{KernelEntry, KernelOps};
pub use regs::GuestRegs;
pub use status::{Error, Status};
pub use table::{Compat, TableHeader};
pub use trap::{TrapEvent, TrapKind};

/// Incompatible layout or semantic changes increment the major version.
pub const ABI_VERSION_MAJOR: u16 = 0;

/// Additive changes (new table entries appended at the end) increment the
/// minor version.
pub const ABI_VERSION_MINOR: u16 = 2;

/// `b"LIU\0"` read as a little-endian `u32`.
pub const ABI_MAGIC: u32 = 0x0055_494c;

/// Opaque reference to a host object (address space, channel, event).
///
/// Handles are allocated by the runtime. Their numeric value carries no
/// meaning on the kernel side and is never a host file descriptor.
pub type Handle = u64;

/// The value of a handle that refers to nothing.
pub const HANDLE_INVALID: Handle = u64::MAX;

/// Memory protection: readable.
pub const PROT_READ: u32 = 0x1;

/// Memory protection: writable.
pub const PROT_WRITE: u32 = 0x2;

/// Memory protection: executable.
pub const PROT_EXEC: u32 = 0x4;

/// All valid protection bits.
pub const PROT_MASK: u32 = PROT_READ | PROT_WRITE | PROT_EXEC;

/// Page size assumed by the version 0 ABI.
pub const PAGE_SIZE: u64 = 4096;
