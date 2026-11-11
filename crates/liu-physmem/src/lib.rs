// SPDX-License-Identifier: MPL-2.0

//! Guest physical memory.
//!
//! Physical memory is one anonymous shared memory object (§5.2). The kernel
//! process maps all of it linearly at `BootInfo::physmem_base`; guest spaces
//! (M2) and device backends (M4) map parts of it. This crate owns the object
//! and the linear map, and checks every physical range before it is used:
//! a physical address that comes from the kernel or a guest is never
//! trusted.

#![forbid(unsafe_code)]

use std::fmt;
use std::io;

use liu_platform::{Mapping, Protection, SharedMemory};

/// Why a physical range was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhysError
{
    /// Empty, or the end overflows.
    Invalid,
    /// The range extends beyond physical memory.
    OutOfRange,
}

impl fmt::Display for PhysError
{
    fn
    fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
        let text: &str = match self
        {
            PhysError::Invalid => "invalid physical range",
            PhysError::OutOfRange => "physical range beyond memory",
        };

        return (f.write_str(text));
    }
}

impl std::error::Error for PhysError
{
}

/// Guest physical memory and its linear map in this process.
#[derive(Debug)]
pub struct Physmem
{
    shm: SharedMemory,
    linear: Mapping,
}

impl Physmem
{
    fn
    from_shm(shm: SharedMemory, addr: Option<u64>) -> io::Result<Physmem>
    {
        let size: usize = usize::try_from(shm.size()).map_err(|_: std::num::TryFromIntError| -> io::Error
        {
            return (io::Error::from(io::ErrorKind::InvalidInput));
        })?;
        let linear: Mapping = match addr
        {
            Some(addr) => shm.map_at(0, size, Protection::READ_WRITE, addr)?,
            None => shm.map(0, size, Protection::READ_WRITE)?,
        };

        return (Ok(Physmem { shm: shm, linear: linear }));
    }

    /// Creates `size` bytes of zeroed physical memory mapped linearly at
    /// exactly `addr`, as the kernel expects.
    pub fn
    create_at(size: u64, addr: u64) -> io::Result<Physmem>
    {
        return (Physmem::from_shm(SharedMemory::create(size)?, Some(addr)));
    }

    /// Creates `size` bytes of zeroed physical memory mapped wherever the
    /// host chooses (tests, tools).
    pub fn
    create(size: u64) -> io::Result<Physmem>
    {
        return (Physmem::from_shm(SharedMemory::create(size)?, None));
    }

    /// Address of the linear map.
    pub fn
    base(&self) -> u64
    {
        return (self.linear.as_ptr() as usize as u64);
    }

    /// Size of physical memory in bytes.
    pub fn
    size(&self) -> u64
    {
        return (self.shm.size());
    }

    /// The backing object, to map parts of it elsewhere.
    pub fn
    shm(&self) -> &SharedMemory
    {
        return (&self.shm);
    }

    /// Checks that `[phys, phys + len)` is a non-empty range inside
    /// physical memory.
    pub fn
    check(&self, phys: u64, len: u64) -> Result<(), PhysError>
    {
        let end: u64 = match phys.checked_add(len)
        {
            Some(end) if len > 0 => end,
            _ =>
            {
                return (Err(PhysError::Invalid));
            }
        };

        if end > self.size()
        {
            return (Err(PhysError::OutOfRange));
        }

        return (Ok(()));
    }

    /// Copies physical memory at `phys` into `buf`.
    pub fn
    read(&self, phys: u64, buf: &mut [u8]) -> Result<(), PhysError>
    {
        self.check(phys, buf.len() as u64)?;
        self.linear.read_at(phys as usize, buf).map_err(|_: io::Error| -> PhysError
        {
            return (PhysError::OutOfRange);
        })?;

        return (Ok(()));
    }

    /// Copies `buf` into physical memory at `phys`.
    pub fn
    write(&mut self, phys: u64, buf: &[u8]) -> Result<(), PhysError>
    {
        self.check(phys, buf.len() as u64)?;
        self.linear.write_at(phys as usize, buf).map_err(|_: io::Error| -> PhysError
        {
            return (PhysError::OutOfRange);
        })?;

        return (Ok(()));
    }
}

#[cfg(test)]
mod tests
{
    use super::*;

    const SIZE: u64 = 1 << 20;

    #[test]
    fn
    reads_back_what_was_written() -> ()
    {
        let mut mem: Physmem = Physmem::create(SIZE).unwrap();
        let mut buf: [u8; 4] = [0; 4];

        mem.write(SIZE - 4, b"tail").unwrap();
        mem.read(SIZE - 4, &mut buf).unwrap();
        assert_eq!(&buf, b"tail");
        assert_eq!(mem.size(), SIZE);
    }

    #[test]
    fn
    refuses_bad_ranges() -> ()
    {
        let mut mem: Physmem = Physmem::create(SIZE).unwrap();
        let mut buf: [u8; 4] = [0; 4];

        assert_eq!(mem.check(0, 0), Err(PhysError::Invalid));
        assert_eq!(mem.check(u64::MAX, 2), Err(PhysError::Invalid));
        assert_eq!(mem.check(SIZE - 4, 5), Err(PhysError::OutOfRange));
        assert_eq!(mem.check(0, SIZE), Ok(()));
        assert_eq!(mem.read(SIZE - 3, &mut buf), Err(PhysError::OutOfRange));
        assert_eq!(mem.write(SIZE, b"x"), Err(PhysError::OutOfRange));
    }

    #[test]
    fn
    other_mappings_see_the_same_memory() -> ()
    {
        let mut mem: Physmem = Physmem::create(SIZE).unwrap();
        let view: Mapping = mem.shm().map(4096, 4096, Protection::READ).unwrap();
        let mut buf: [u8; 2] = [0; 2];

        mem.write(4096 + 8, b"ok").unwrap();
        view.read_at(8, &mut buf).unwrap();
        assert_eq!(&buf, b"ok");
    }
}
