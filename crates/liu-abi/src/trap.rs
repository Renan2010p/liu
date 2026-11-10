// SPDX-License-Identifier: MPL-2.0

//! Events that stop guest execution and return control to the kernel.

use crate::status::Error;

/// The guest executed a system call instruction. The call was not executed
/// by the host. `TrapEvent::code` holds the call number from `rax` at entry.
pub const TRAP_SYSCALL: u32 = 1;

/// The guest faulted on a memory access. `addr` and `access` describe it.
pub const TRAP_PAGE_FAULT: u32 = 2;

/// The guest received a host signal that is not a page fault (illegal
/// instruction, breakpoint, arithmetic fault). `signo` and `code` hold the
/// architectural reason.
pub const TRAP_SIGNAL: u32 = 3;

/// The guest address space no longer exists (the host process died).
pub const TRAP_EXITED: u32 = 4;

/// Access bit: the faulting access was a read.
pub const ACCESS_READ: u32 = 0x1;

/// Access bit: the faulting access was a write.
pub const ACCESS_WRITE: u32 = 0x2;

/// Access bit: the faulting access was an instruction fetch.
pub const ACCESS_EXEC: u32 = 0x4;

/// Signal numbers carried in `TrapEvent::signo`. These are LIU's own
/// numbers; neither host nor Linux numbering crosses the boundary.
pub const TRAP_SIG_ILL: u32 = 1;
pub const TRAP_SIG_TRAP: u32 = 2;
pub const TRAP_SIG_FPE: u32 = 3;
pub const TRAP_SIG_BUS: u32 = 4;

/// Written by the runtime when `as_run` returns.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrapEvent
{
    /// One of the `TRAP_*` kinds.
    pub kind: u32,
    /// `TRAP_SIG_*` for `TRAP_SIGNAL`, zero otherwise.
    pub signo: u32,
    /// `ACCESS_*` bits for `TRAP_PAGE_FAULT`, zero otherwise.
    pub access: u32,
    pub reserved: u32,
    /// Faulting address for `TRAP_PAGE_FAULT`.
    pub addr: u64,
    /// System call number for `TRAP_SYSCALL`, exit status for `TRAP_EXITED`.
    pub code: u64,
}

const _: () = assert!(size_of::<TrapEvent>() == 32);

/// Validated Rust view of `TrapEvent::kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrapKind
{
    Syscall,
    PageFault,
    Signal,
    Exited,
}

impl TrapKind
{
    pub const fn
    from_raw(kind: u32) -> Result<TrapKind, Error>
    {
        let trap: TrapKind = match kind
        {
            TRAP_SYSCALL => TrapKind::Syscall,
            TRAP_PAGE_FAULT => TrapKind::PageFault,
            TRAP_SIGNAL => TrapKind::Signal,
            TRAP_EXITED => TrapKind::Exited,
            _ =>
            {
                return (Err(Error::Invalid));
            }
        };

        return (Ok(trap));
    }

    pub const fn
    raw(self) -> u32
    {
        let kind: u32 = match self
        {
            TrapKind::Syscall => TRAP_SYSCALL,
            TrapKind::PageFault => TRAP_PAGE_FAULT,
            TrapKind::Signal => TRAP_SIGNAL,
            TrapKind::Exited => TRAP_EXITED,
        };

        return (kind);
    }
}

#[cfg(test)]
mod tests
{
    use super::*;

    #[test]
    fn
    kinds_round_trip() -> ()
    {
        let kinds: [TrapKind; 4] =
            [TrapKind::Syscall, TrapKind::PageFault, TrapKind::Signal, TrapKind::Exited];

        for kind in kinds
        {
            assert_eq!(TrapKind::from_raw(kind.raw()), Ok(kind));
        }
    }

    #[test]
    fn
    rejects_unknown_kind() -> ()
    {
        assert_eq!(TrapKind::from_raw(0), Err(Error::Invalid));
        assert_eq!(TrapKind::from_raw(99), Err(Error::Invalid));
    }
}
