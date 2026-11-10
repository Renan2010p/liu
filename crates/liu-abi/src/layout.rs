// SPDX-License-Identifier: MPL-2.0

//! Machine-readable layout of every boundary type.
//!
//! `cargo xtask check-abi` compiles a C program against the generated header
//! that prints the same facts, and compares the two. Any difference means the
//! header and the Rust definitions disagree.

use core::mem::offset_of;

use crate::boot::BootInfo;
use crate::host_ops::HostOps;
use crate::kernel_ops::KernelOps;
use crate::regs::GuestRegs;
use crate::table::TableHeader;
use crate::trap::TrapEvent;

/// Layout of one `#[repr(C)]` type.
#[derive(Clone, Copy, Debug)]
pub struct TypeLayout
{
    /// Name of the type in the generated C header.
    pub c_name: &'static str,
    pub size: usize,
    pub align: usize,
    /// `(field, offset)` pairs in declaration order.
    pub fields: &'static [(&'static str, usize)],
}

macro_rules! layout
{
    ($ty:ty, $c_name:literal, [$($field:ident),* $(,)?]) =>
    {
        TypeLayout
        {
            c_name: $c_name,
            size: size_of::<$ty>(),
            align: align_of::<$ty>(),
            fields: &[$((stringify!($field), offset_of!($ty, $field))),*],
        }
    };
}

/// Every type that crosses the boundary.
pub const TYPES: &[TypeLayout] = &[
    layout!(TableHeader, "liu_table_header", [magic, abi_major, abi_minor, size, reserved]),
    layout!(
        GuestRegs,
        "liu_guest_regs",
        [
            rax, rbx, rcx, rdx, rsi, rdi, rbp, rsp, r8, r9, r10, r11, r12, r13, r14, r15, rip,
            rflags, fs_base, gs_base,
        ]
    ),
    layout!(TrapEvent, "liu_trap_event", [kind, signo, access, reserved, addr, code]),
    layout!(
        BootInfo,
        "liu_boot_info",
        [
            header,
            physmem_base,
            physmem_size,
            physmem_handle,
            image_base,
            image_size,
            cmdline,
            cmdline_len,
            initrd_offset,
            initrd_size,
            fpu_state_size,
            page_size,
        ]
    ),
    layout!(
        HostOps,
        "liu_host_ops",
        [
            header,
            log_write,
            panic,
            kmap,
            kunmap,
            kprotect,
            as_create,
            as_destroy,
            as_map,
            as_unmap,
            as_protect,
            as_flush,
            as_run,
            as_get_fpu,
            as_set_fpu,
            irq_enable,
            irq_disable,
            irq_save,
            irq_restore,
            idle,
            clock_monotonic,
            clock_realtime,
            timer_set,
            timer_cancel,
            kstack_alloc,
            kstack_free,
            get_random,
            dev_connect,
            chan_send,
            chan_recv,
            event_create,
            event_signal,
            irq_bind,
            handle_close,
        ]
    ),
    layout!(KernelOps, "liu_kernel_ops", [header, irq, timer, kernel_fault, shutdown]),
];

#[cfg(test)]
mod tests
{
    use super::*;
    use crate::host_ops::HOST_OPS_ENTRIES;
    use crate::kernel_ops::KERNEL_OPS_ENTRIES;

    fn
    find(name: &str) -> &'static TypeLayout
    {
        for layout in TYPES
        {
            if layout.c_name == name
            {
                return (layout);
            }
        }
        panic!("no layout for {}", name);
    }

    #[test]
    fn
    tables_list_every_entry() -> ()
    {
        assert_eq!(find("liu_host_ops").fields.len(), 1 + HOST_OPS_ENTRIES);
        assert_eq!(find("liu_kernel_ops").fields.len(), 1 + KERNEL_OPS_ENTRIES);
    }

    #[test]
    fn
    fields_are_in_increasing_order() -> ()
    {
        for layout in TYPES
        {
            let mut previous: usize = 0;

            for (index, (name, offset)) in layout.fields.iter().enumerate()
            {
                if index > 0
                {
                    assert!(*offset > previous, "{}.{} is out of order", layout.c_name, name);
                }
                previous = *offset;
            }
        }
    }
}
