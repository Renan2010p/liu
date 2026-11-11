// SPDX-License-Identifier: MPL-2.0

//! Anonymous shared memory: the backing object for guest physical memory.

use std::io;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::ptr::{self, NonNull};

use crate::host;
use crate::sys;
use crate::vm;

/// Memory protection for a mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Protection(u32);

impl Protection
{
    pub const NONE: Protection = Protection(0);
    pub const READ: Protection = Protection(0x1);
    pub const WRITE: Protection = Protection(0x2);
    pub const EXEC: Protection = Protection(0x4);
    pub const READ_WRITE: Protection = Protection(0x1 | 0x2);

    /// Combines two protections.
    pub const fn
    union(self, other: Protection) -> Protection
    {
        return (Protection(self.0 | other.0));
    }

    /// Returns whether every bit of `other` is set in `self`.
    pub const fn
    contains(self, other: Protection) -> bool
    {
        return (self.0 & other.0 == other.0);
    }

    pub(crate) fn
    to_host(self) -> libc::c_int
    {
        let mut prot: libc::c_int = libc::PROT_NONE;

        if self.contains(Protection::READ)
        {
            prot |= libc::PROT_READ;
        }
        if self.contains(Protection::WRITE)
        {
            prot |= libc::PROT_WRITE;
        }
        if self.contains(Protection::EXEC)
        {
            prot |= libc::PROT_EXEC;
        }

        return (prot);
    }
}

/// An anonymous shared memory object of fixed size.
///
/// The object has no name in any namespace. It can be shared with another
/// process only by passing its descriptor.
#[derive(Debug)]
pub struct SharedMemory
{
    fd: OwnedFd,
    size: u64,
}

impl SharedMemory
{
    /// Creates a zero-filled object of `size` bytes.
    pub fn
    create(size: u64) -> io::Result<SharedMemory>
    {
        if size == 0
        {
            return (Err(io::Error::from(io::ErrorKind::InvalidInput)));
        }

        let length: libc::off_t = match libc::off_t::try_from(size)
        {
            Ok(length) => length,
            Err(_) =>
            {
                return (Err(io::Error::from(io::ErrorKind::InvalidInput)));
            }
        };
        let fd: OwnedFd = host::shm_create_anonymous()?;

        // SAFETY: `fd` is a valid shared memory descriptor we own.
        sys::check(unsafe { libc::ftruncate(fd.as_raw_fd(), length) })?;

        return (Ok(SharedMemory { fd: fd, size: size }));
    }

    /// Size of the object in bytes.
    pub fn
    size(&self) -> u64
    {
        return (self.size);
    }

    /// Maps `len` bytes starting at `offset` into this process.
    pub fn
    map(&self, offset: u64, len: usize, prot: Protection) -> io::Result<Mapping>
    {
        let end: Option<u64> = offset.checked_add(len as u64);

        if len == 0 || end.is_none() || end.unwrap_or(u64::MAX) > self.size
        {
            return (Err(io::Error::from(io::ErrorKind::InvalidInput)));
        }

        let host_offset: libc::off_t = match libc::off_t::try_from(offset)
        {
            Ok(host_offset) => host_offset,
            Err(_) =>
            {
                return (Err(io::Error::from(io::ErrorKind::InvalidInput)));
            }
        };

        // SAFETY: a fresh mapping at a kernel-chosen address cannot alias any
        // existing Rust object; the arguments were validated above.
        let addr: *mut libc::c_void = unsafe
        {
            libc::mmap(
                ptr::null_mut(),
                len,
                prot.to_host(),
                libc::MAP_SHARED,
                self.fd.as_raw_fd(),
                host_offset,
            )
        };

        if addr == libc::MAP_FAILED
        {
            return (Err(io::Error::last_os_error()));
        }

        let base: NonNull<u8> = match NonNull::new(addr.cast::<u8>())
        {
            Some(base) => base,
            None =>
            {
                return (Err(io::Error::other("mmap returned a null mapping")));
            }
        };

        return (Ok(Mapping { base: base, len: len }));
    }
    /// Maps `len` bytes starting at `offset` at exactly `addr`.
    ///
    /// Fails with `AddrInUse` if any part of the range is already mapped.
    pub fn
    map_at(&self, offset: u64, len: usize, prot: Protection, addr: u64) -> io::Result<Mapping>
    {
        let end: Option<u64> = offset.checked_add(len as u64);

        if len == 0 || end.is_none() || end.unwrap_or(u64::MAX) > self.size
        {
            return (Err(io::Error::from(io::ErrorKind::InvalidInput)));
        }

        let host_offset: libc::off_t = match libc::off_t::try_from(offset)
        {
            Ok(host_offset) => host_offset,
            Err(_) =>
            {
                return (Err(io::Error::from(io::ErrorKind::InvalidInput)));
            }
        };
        let base: NonNull<u8> =
            vm::map_exact(addr, len, prot, libc::MAP_SHARED, self.fd.as_raw_fd(), host_offset)?;

        return (Ok(Mapping { base: base, len: len }));
    }
}

impl AsFd for SharedMemory
{
    fn
    as_fd(&self) -> BorrowedFd<'_>
    {
        return (self.fd.as_fd());
    }
}

/// A mapping of part of a [`SharedMemory`] object, unmapped on drop.
///
/// The memory may be modified concurrently by other processes, so it is never
/// exposed as a Rust slice. Access goes through bounds-checked copies.
#[derive(Debug)]
pub struct Mapping
{
    base: NonNull<u8>,
    len: usize,
}

// SAFETY: the mapping is exclusively owned by this value; the pointer is not
// tied to the creating thread.
unsafe impl Send for Mapping
{
}

impl Mapping
{
    /// Length of the mapping in bytes.
    pub fn
    len(&self) -> usize
    {
        return (self.len);
    }

    /// Always false: empty mappings cannot be created.
    pub fn
    is_empty(&self) -> bool
    {
        return (self.len == 0);
    }

    /// Start address of the mapping in this process.
    pub fn
    as_ptr(&self) -> *mut u8
    {
        return (self.base.as_ptr());
    }

    fn
    check_range(&self, offset: usize, len: usize) -> io::Result<()>
    {
        let end: Option<usize> = offset.checked_add(len);

        if end.is_none() || end.unwrap_or(usize::MAX) > self.len
        {
            return (Err(io::Error::from(io::ErrorKind::InvalidInput)));
        }

        return (Ok(()));
    }

    /// Copies bytes out of the mapping.
    pub fn
    read_at(&self, offset: usize, buf: &mut [u8]) -> io::Result<()>
    {
        self.check_range(offset, buf.len())?;

        // SAFETY: the range lies within the mapping, and `buf` is a distinct
        // Rust allocation.
        unsafe
        {
            ptr::copy_nonoverlapping(self.base.as_ptr().add(offset), buf.as_mut_ptr(), buf.len());
        }

        return (Ok(()));
    }

    /// Copies bytes into the mapping.
    pub fn
    write_at(&mut self, offset: usize, buf: &[u8]) -> io::Result<()>
    {
        self.check_range(offset, buf.len())?;

        // SAFETY: the range lies within the mapping, and `buf` is a distinct
        // Rust allocation.
        unsafe
        {
            ptr::copy_nonoverlapping(buf.as_ptr(), self.base.as_ptr().add(offset), buf.len());
        }

        return (Ok(()));
    }
}

impl Drop for Mapping
{
    fn
    drop(&mut self) -> ()
    {
        // SAFETY: `base` and `len` describe a mapping this value owns.
        unsafe
        {
            libc::munmap(self.base.as_ptr().cast::<libc::c_void>(), self.len);
        }
    }
}
