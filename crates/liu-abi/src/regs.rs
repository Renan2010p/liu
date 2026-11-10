// SPDX-License-Identifier: MPL-2.0

//! Guest register file.

/// x86_64 user-mode register state of a guest thread.
///
/// This is the architectural register set, not a Linux structure. The
/// runtime fills it from the host on every trap and loads it back before
/// resuming the guest.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GuestRegs
{
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rsp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,
    pub rflags: u64,
    pub fs_base: u64,
    pub gs_base: u64,
}

const _: () = assert!(size_of::<GuestRegs>() == 20 * 8);
