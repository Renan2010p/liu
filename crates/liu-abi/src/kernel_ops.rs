// SPDX-License-Identifier: MPL-2.0

//! Entry points the kernel registers for the runtime to call.

use crate::boot::BootInfo;
use crate::host_ops::HostOps;
use crate::status::Status;
use crate::table::TableHeader;

/// Filled in by the kernel during `liu_kernel_entry`, before it enables
/// interrupts. Every entry is called on the vCPU thread.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct KernelOps
{
    pub header: TableHeader,
    /// Delivers the interrupts whose bits are set in `pending`.
    pub irq: Option<unsafe extern "C" fn(pending: u64) -> ()>,
    /// The one-shot timer armed with `timer_set` expired.
    pub timer: Option<unsafe extern "C" fn() -> ()>,
    /// A fault occurred while the vCPU was executing kernel code.
    pub kernel_fault: Option<unsafe extern "C" fn(addr: u64, access: u32, ip: u64) -> Status>,
    /// The supervisor requests an orderly shutdown.
    pub shutdown: Option<unsafe extern "C" fn(reason: u32) -> ()>,
}

/// Number of entries in a version 0.1 `KernelOps`.
pub const KERNEL_OPS_ENTRIES: usize = 4;

const _: () = assert!(size_of::<KernelOps>() == 16 + KERNEL_OPS_ENTRIES * 8);

/// Shutdown reason: power off.
pub const SHUTDOWN_POWEROFF: u32 = 1;

/// Shutdown reason: reboot.
pub const SHUTDOWN_REBOOT: u32 = 2;

/// Signature of `liu_kernel_entry`, the ELF entry point of the kernel image.
///
/// The runtime passes a pre-sized `KernelOps` whose header carries the
/// runtime's ABI version; the kernel fills in its entries and its own header.
/// The function returns only on shutdown.
pub type KernelEntry = unsafe extern "C" fn(
    host: *const HostOps,
    kernel: *mut KernelOps,
    boot: *const BootInfo,
) -> Status;

impl KernelOps
{
    /// A table with a valid header and no entries.
    pub const fn
    empty() -> KernelOps
    {
        let ops: KernelOps = KernelOps
        {
            header: TableHeader::new(size_of::<KernelOps>()),
            irq: None,
            timer: None,
            kernel_fault: None,
            shutdown: None,
        };

        return (ops);
    }
}
