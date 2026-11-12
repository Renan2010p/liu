// SPDX-License-Identifier: MPL-2.0

//! Guest address spaces: the `as_*` entries of the host ABI.
//!
//! Each space is a [`GuestSpace`] of the host's trap backend, named by a
//! handle the kernel receives from `as_create`. Arguments come from the
//! kernel and are checked here (protection bits, handles) and again by the
//! platform layer (ranges, physical bounds) before reaching the host.

use std::collections::HashMap;
use std::io;

use liu_abi::{Error, GuestRegs, Handle, TrapEvent, HANDLE_INVALID, PROT_EXEC, PROT_MASK, PROT_READ, PROT_WRITE};
use liu_platform::{GuestSpace, Protection, SharedMemory, TrapBackend};

/// Most guest spaces alive at once.
pub const MAX_SPACES: usize = 4096;

/// Converts ABI protection bits. Unlike kernel mappings, guest mappings may
/// be writable and executable at once: Linux allows it to user programs
/// (JIT compilers), and the guest is isolated from the runtime anyway.
fn
protection(prot: u32) -> Result<Protection, Error>
{
    if prot & !PROT_MASK != 0
    {
        return (Err(Error::Invalid));
    }

    let mut out: Protection = Protection::NONE;

    if prot & PROT_READ != 0
    {
        out = out.union(Protection::READ);
    }
    if prot & PROT_WRITE != 0
    {
        out = out.union(Protection::WRITE);
    }
    if prot & PROT_EXEC != 0
    {
        out = out.union(Protection::EXEC);
    }

    return (Ok(out));
}

/// Maps a platform error to an ABI error.
fn
host_error(error: io::Error) -> Error
{
    let mapped: Error = match error.kind()
    {
        io::ErrorKind::InvalidInput => Error::Invalid,
        io::ErrorKind::Unsupported => Error::Unsupported,
        io::ErrorKind::OutOfMemory => Error::NoMemory,
        _ => Error::Io,
    };

    return (mapped);
}

/// The guest spaces of one kernel.
#[derive(Debug)]
pub struct Spaces
{
    /// `None` on hosts without an implemented trap backend.
    backend: Option<Box<dyn TrapBackend>>,
    physmem: SharedMemory,
    table: HashMap<Handle, Box<dyn GuestSpace>>,
    next: Handle,
}

impl Spaces
{
    /// Spaces backed by `physmem`, created with `backend`.
    pub fn
    new(backend: Option<Box<dyn TrapBackend>>, physmem: SharedMemory) -> Spaces
    {
        return (Spaces { backend: backend, physmem: physmem, table: HashMap::new(), next: 1 });
    }

    /// Name of the trap backend, for the log.
    pub fn
    backend_name(&self) -> String
    {
        let name: String = match &self.backend
        {
            Some(backend) => format!("{:?}", backend.kind()),
            None => String::from("none"),
        };

        return (name);
    }

    fn
    space(&mut self, handle: Handle) -> Result<&mut Box<dyn GuestSpace>, Error>
    {
        return (self.table.get_mut(&handle).ok_or(Error::BadHandle));
    }

    /// `as_create`.
    pub fn
    create(&mut self) -> Result<Handle, Error>
    {
        if self.table.len() >= MAX_SPACES
        {
            return (Err(Error::NoMemory));
        }

        let backend: &mut Box<dyn TrapBackend> = self.backend.as_mut().ok_or(Error::Unsupported)?;
        let space: Box<dyn GuestSpace> = backend.create_space(&self.physmem).map_err(host_error)?;
        let handle: Handle = self.next;

        self.next += 1;
        if self.next == HANDLE_INVALID
        {
            self.next = 1;
        }
        self.table.insert(handle, space);
        return (Ok(handle));
    }

    /// `as_destroy`: kills the host process.
    pub fn
    destroy(&mut self, handle: Handle) -> Result<(), Error>
    {
        self.table.remove(&handle).ok_or(Error::BadHandle)?;
        return (Ok(()));
    }

    /// `as_map`.
    pub fn
    map(&mut self, handle: Handle, vaddr: u64, phys: u64, len: u64, prot: u32) -> Result<(), Error>
    {
        let prot: Protection = protection(prot)?;

        return (self.space(handle)?.map(vaddr, len, phys, prot).map_err(host_error));
    }

    /// `as_unmap`.
    pub fn
    unmap(&mut self, handle: Handle, vaddr: u64, len: u64) -> Result<(), Error>
    {
        return (self.space(handle)?.unmap(vaddr, len).map_err(host_error));
    }

    /// `as_protect`.
    pub fn
    protect(&mut self, handle: Handle, vaddr: u64, len: u64, prot: u32) -> Result<(), Error>
    {
        let prot: Protection = protection(prot)?;

        return (self.space(handle)?.protect(vaddr, len, prot).map_err(host_error));
    }

    /// `as_flush`.
    pub fn
    flush(&mut self, handle: Handle) -> Result<(), Error>
    {
        return (self.space(handle)?.flush().map_err(host_error));
    }

    /// `as_run`: runs guest code until the next trap.
    pub fn
    run(&mut self, handle: Handle, regs: &mut GuestRegs) -> Result<TrapEvent, Error>
    {
        return (self.space(handle)?.run(regs).map_err(host_error));
    }

    /// Number of live spaces.
    pub fn
    len(&self) -> usize
    {
        return (self.table.len());
    }

    /// Returns whether no space is alive.
    pub fn
    is_empty(&self) -> bool
    {
        return (self.table.is_empty());
    }
}

#[cfg(test)]
mod tests
{
    use super::*;

    fn
    spaces() -> Spaces
    {
        return (Spaces::new(None, SharedMemory::create(1 << 20).unwrap()));
    }

    #[test]
    fn
    guest_mappings_may_be_writable_and_executable() -> ()
    {
        assert!(protection(PROT_READ | PROT_WRITE | PROT_EXEC).is_ok());
        assert_eq!(protection(0x10), Err(Error::Invalid));
    }

    #[test]
    fn
    without_a_backend_spaces_are_unsupported() -> ()
    {
        let mut s: Spaces = spaces();

        assert_eq!(s.create(), Err(Error::Unsupported));
        assert_eq!(s.backend_name(), "none");
    }

    #[test]
    fn
    unknown_handles_are_refused() -> ()
    {
        let mut s: Spaces = spaces();
        let mut regs: GuestRegs = GuestRegs::default();

        assert_eq!(s.destroy(7), Err(Error::BadHandle));
        assert_eq!(s.map(7, 0x1000, 0, 0x1000, PROT_READ), Err(Error::BadHandle));
        assert_eq!(s.flush(HANDLE_INVALID), Err(Error::BadHandle));
        assert_eq!(s.run(7, &mut regs), Err(Error::BadHandle));
        assert!(s.is_empty());
    }
}
