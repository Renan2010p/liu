/* SPDX-License-Identifier: MPL-2.0 */

/*
 * Layout of the ABI test kernel. It is linked at the address arch/um uses
 * (START = 0x60000000), so the runtime is exercised with the same placement
 * the Linux kernel image will have.
 */

ENTRY(liu_kernel_entry)

SECTIONS
{
    . = 0x60000000;

    .text : ALIGN(4096)
    {
        *(.text .text.*)
    }

    .rodata : ALIGN(4096)
    {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(4096)
    {
        *(.data .data.*)
        *(.got .got.*)
    }

    .bss : ALIGN(4096)
    {
        *(.bss .bss.*)
        *(COMMON)
    }

    /DISCARD/ :
    {
        *(.eh_frame .eh_frame_hdr .comment .note .note.*)
    }
}
