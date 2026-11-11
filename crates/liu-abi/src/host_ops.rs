// SPDX-License-Identifier: MPL-2.0

//! Services the runtime provides to the kernel.
//!
//! Rules that apply to every entry:
//!
//! - Entries may be null when the runtime does not provide them; check with
//!   [`provides!`](crate::provides) before calling.
//! - Pointers are addresses in the kernel process and must stay valid for
//!   the duration of the call.
//! - Entries are called only from the vCPU thread unless stated otherwise.
//! - No entry names a host path, a host socket address or a Linux structure.
//!
//! New entries are appended at the end and increment `ABI_VERSION_MINOR`.

use crate::regs::GuestRegs;
use crate::status::Status;
use crate::table::TableHeader;
use crate::trap::TrapEvent;
use crate::Handle;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct HostOps
{
    pub header: TableHeader,

    // Console and logging.
    /// Writes early boot output. May be called from any context.
    pub log_write: Option<unsafe extern "C" fn(buf: *const u8, len: usize) -> ()>,
    /// Stops the instance after a fatal kernel error. Does not return.
    pub panic: Option<unsafe extern "C" fn(msg: *const u8, len: usize) -> ()>,

    // Kernel address space.
    /// Maps `len` bytes of physical memory at `phys` to kernel address
    /// `vaddr`, replacing any previous mapping.
    ///
    /// The range must lie in the `kmap` window of `BootInfo`, and every
    /// argument must be page aligned (`STATUS_INVALID`). A range outside the
    /// window or beyond physical memory is `STATUS_FAULT`; writable and
    /// executable at once is `STATUS_DENIED`.
    pub kmap: Option<unsafe extern "C" fn(vaddr: u64, phys: u64, len: u64, prot: u32) -> Status>,
    /// Makes a window range inaccessible again. Unmapped pages are ignored.
    pub kunmap: Option<unsafe extern "C" fn(vaddr: u64, len: u64) -> Status>,
    /// Changes the protection of mapped window pages; any unmapped page in
    /// the range is `STATUS_FAULT`. Same rules as `kmap` otherwise.
    pub kprotect: Option<unsafe extern "C" fn(vaddr: u64, len: u64, prot: u32) -> Status>,

    // Guest address spaces. Changes may be queued until `as_flush` or the
    // next `as_run` on the same space.
    pub as_create: Option<unsafe extern "C" fn(out: *mut Handle) -> Status>,
    pub as_destroy: Option<unsafe extern "C" fn(space: Handle) -> Status>,
    pub as_map: Option<
        unsafe extern "C" fn(space: Handle, vaddr: u64, phys: u64, len: u64, prot: u32) -> Status,
    >,
    pub as_unmap: Option<unsafe extern "C" fn(space: Handle, vaddr: u64, len: u64) -> Status>,
    pub as_protect:
        Option<unsafe extern "C" fn(space: Handle, vaddr: u64, len: u64, prot: u32) -> Status>,
    pub as_flush: Option<unsafe extern "C" fn(space: Handle) -> Status>,

    // Guest execution.
    /// Loads `regs`, runs guest code in `space` until the next trap, stores
    /// the resulting registers back into `regs` and describes the trap in
    /// `event`.
    pub as_run: Option<
        unsafe extern "C" fn(space: Handle, regs: *mut GuestRegs, event: *mut TrapEvent) -> Status,
    >,
    pub as_get_fpu: Option<unsafe extern "C" fn(space: Handle, buf: *mut u8, len: usize) -> Status>,
    pub as_set_fpu:
        Option<unsafe extern "C" fn(space: Handle, buf: *const u8, len: usize) -> Status>,

    // Virtual CPU.
    pub irq_enable: Option<unsafe extern "C" fn() -> ()>,
    pub irq_disable: Option<unsafe extern "C" fn() -> ()>,
    /// Disables interrupts and returns the previous state.
    pub irq_save: Option<unsafe extern "C" fn() -> u64>,
    pub irq_restore: Option<unsafe extern "C" fn(flags: u64) -> ()>,
    /// Blocks until an interrupt is pending. Returns with interrupts enabled.
    pub idle: Option<unsafe extern "C" fn() -> ()>,

    // Time. Values are nanoseconds.
    pub clock_monotonic: Option<unsafe extern "C" fn() -> u64>,
    pub clock_realtime: Option<unsafe extern "C" fn() -> u64>,
    /// Arms the one-shot timer at an absolute monotonic deadline.
    pub timer_set: Option<unsafe extern "C" fn(deadline: u64) -> Status>,
    pub timer_cancel: Option<unsafe extern "C" fn() -> ()>,

    // Kernel thread stacks.
    /// Allocates a zero-filled stack of `size` bytes (a page multiple) with
    /// an inaccessible guard page below it, and stores its lowest usable
    /// address in `out`. The stack grows down from `*out + size`.
    pub kstack_alloc: Option<unsafe extern "C" fn(size: u64, out: *mut u64) -> Status>,
    /// Frees a stack returned by `kstack_alloc`, given the same address and
    /// size. Anything else is ignored.
    pub kstack_free: Option<unsafe extern "C" fn(addr: u64, size: u64) -> ()>,

    // Entropy.
    pub get_random: Option<unsafe extern "C" fn(buf: *mut u8, len: usize) -> Status>,

    // Devices. Names are assigned by the supervisor configuration.
    pub dev_connect:
        Option<unsafe extern "C" fn(name: *const u8, len: usize, out: *mut Handle) -> Status>,
    pub chan_send: Option<
        unsafe extern "C" fn(
            chan: Handle,
            buf: *const u8,
            len: usize,
            handles: *const Handle,
            nhandles: usize,
        ) -> Status,
    >,
    pub chan_recv: Option<
        unsafe extern "C" fn(
            chan: Handle,
            buf: *mut u8,
            cap: usize,
            out_len: *mut usize,
            handles: *mut Handle,
            handles_cap: usize,
            out_nhandles: *mut usize,
        ) -> Status,
    >,
    pub event_create: Option<unsafe extern "C" fn(out: *mut Handle) -> Status>,
    pub event_signal: Option<unsafe extern "C" fn(event: Handle) -> Status>,
    /// Raises interrupt `irq` whenever `handle` becomes readable.
    pub irq_bind: Option<unsafe extern "C" fn(handle: Handle, irq: u32) -> Status>,
    pub handle_close: Option<unsafe extern "C" fn(handle: Handle) -> Status>,
}

/// Number of entries in a version 0.1 `HostOps`.
pub const HOST_OPS_ENTRIES: usize = 33;

const _: () = assert!(size_of::<HostOps>() == 16 + HOST_OPS_ENTRIES * 8);

impl HostOps
{
    /// A table with a valid header and no entries.
    pub const fn
    empty() -> HostOps
    {
        let ops: HostOps = HostOps
        {
            header: TableHeader::new(size_of::<HostOps>()),
            log_write: None,
            panic: None,
            kmap: None,
            kunmap: None,
            kprotect: None,
            as_create: None,
            as_destroy: None,
            as_map: None,
            as_unmap: None,
            as_protect: None,
            as_flush: None,
            as_run: None,
            as_get_fpu: None,
            as_set_fpu: None,
            irq_enable: None,
            irq_disable: None,
            irq_save: None,
            irq_restore: None,
            idle: None,
            clock_monotonic: None,
            clock_realtime: None,
            timer_set: None,
            timer_cancel: None,
            kstack_alloc: None,
            kstack_free: None,
            get_random: None,
            dev_connect: None,
            chan_send: None,
            chan_recv: None,
            event_create: None,
            event_signal: None,
            irq_bind: None,
            handle_close: None,
        };

        return (ops);
    }
}

#[cfg(test)]
mod tests
{
    use super::*;
    use crate::provides;

    extern "C" fn
    fake_clock() -> u64
    {
        return (42);
    }

    #[test]
    fn
    empty_table_provides_nothing() -> ()
    {
        let ops: HostOps = HostOps::empty();

        assert!(!provides!(ops, HostOps, clock_monotonic));
        assert!(!provides!(ops, HostOps, handle_close));
    }

    #[test]
    fn
    provides_set_entries() -> ()
    {
        let mut ops: HostOps = HostOps::empty();

        ops.clock_monotonic = Some(fake_clock);
        assert!(provides!(ops, HostOps, clock_monotonic));
    }

    #[test]
    fn
    entries_beyond_declared_size_are_absent() -> ()
    {
        let mut ops: HostOps = HostOps::empty();

        ops.handle_close = Some(fake_handle_close);
        ops.header.size = core::mem::offset_of!(HostOps, handle_close) as u32;
        assert!(!provides!(ops, HostOps, handle_close));
    }

    extern "C" fn
    fake_handle_close(_handle: Handle) -> Status
    {
        return (0);
    }
}
