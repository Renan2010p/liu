// SPDX-License-Identifier: MPL-2.0

//! Information the runtime hands to the kernel at entry.

use crate::table::TableHeader;
use crate::Handle;

/// Boot parameters, valid for the lifetime of the kernel.
///
/// All pointers are addresses in the kernel process. Guest physical memory is
/// mapped contiguously at `physmem_base` before entry.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BootInfo
{
    pub header: TableHeader,
    /// Kernel virtual address where guest physical memory starts.
    pub physmem_base: u64,
    /// Size of guest physical memory in bytes, a multiple of the page size.
    pub physmem_size: u64,
    /// Handle of the physical memory object, passed to device backends.
    pub physmem_handle: Handle,
    /// Lowest address of the loaded kernel image.
    pub image_base: u64,
    /// Size of the loaded kernel image, page aligned.
    pub image_size: u64,
    /// Kernel command line, not NUL terminated.
    pub cmdline: *const u8,
    pub cmdline_len: u64,
    /// Offset of the initial ramdisk in physical memory, or zero.
    pub initrd_offset: u64,
    /// Size of the initial ramdisk, or zero when absent.
    pub initrd_size: u64,
    /// Size of the buffer `as_get_fpu` and `as_set_fpu` expect.
    pub fpu_state_size: u64,
    /// Host page size. Version 0 requires `PAGE_SIZE`.
    pub page_size: u64,
    /// Start of the window where `kmap` may map physical memory (ABI 0.2).
    /// The window is page aligned and inaccessible until mapped.
    pub kmap_base: u64,
    /// Size of the `kmap` window in bytes (ABI 0.2).
    pub kmap_size: u64,
}

const _: () = assert!(size_of::<BootInfo>() == 16 + 13 * 8);
