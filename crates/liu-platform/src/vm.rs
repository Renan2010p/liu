// SPDX-License-Identifier: MPL-2.0

//! Memory placed at fixed addresses: the kernel image and guest physical
//! memory must appear exactly where the kernel was linked to find them.

use std::io;
use std::ptr::{self, NonNull};

use crate::host;
use crate::shm::Protection;

/// Returns the host page size in bytes.
pub fn
page_size() -> usize
{
    // SAFETY: sysconf has no preconditions.
    let size: libc::c_long = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };

    if size <= 0
    {
        return (4096);
    }

    return (size as usize);
}

/// Maps `len` bytes at exactly `addr`, never replacing an existing mapping.
///
/// Hosts without a "fixed, no replace" flag treat `addr` as a hint; the
/// result is checked and refused if the host placed it elsewhere.
pub(crate) fn
map_exact(
    addr: u64,
    len: usize,
    prot: Protection,
    flags: libc::c_int,
    fd: libc::c_int,
    offset: libc::off_t,
) -> io::Result<NonNull<u8>>
{
    let page: u64 = page_size() as u64;

    if addr == 0 || len == 0 || addr % page != 0 || addr.checked_add(len as u64).is_none()
    {
        return (Err(io::Error::from(io::ErrorKind::InvalidInput)));
    }

    // SAFETY: the "no replace" flag guarantees the call cannot clobber an
    // existing mapping, so no Rust object can be aliased; arguments were
    // validated above.
    let got: *mut libc::c_void = unsafe
    {
        libc::mmap(
            addr as usize as *mut libc::c_void,
            len,
            prot.to_host(),
            flags | host::MAP_FIXED_NOREPLACE,
            fd,
            offset,
        )
    };

    if got == libc::MAP_FAILED
    {
        let error: io::Error = io::Error::last_os_error();

        if error.raw_os_error() == Some(host::MAP_IN_USE_ERRNO)
        {
            return (Err(io::Error::from(io::ErrorKind::AddrInUse)));
        }
        return (Err(error));
    }
    if got as usize as u64 != addr
    {
        // SAFETY: `got` and `len` describe the mapping just created.
        unsafe
        {
            libc::munmap(got, len);
        }
        return (Err(io::Error::from(io::ErrorKind::AddrInUse)));
    }

    let base: NonNull<u8> = match NonNull::new(got.cast::<u8>())
    {
        Some(base) => base,
        None =>
        {
            return (Err(io::Error::other("mmap returned a null mapping")));
        }
    };

    return (Ok(base));
}

/// Private anonymous memory at a fixed address, unmapped on drop.
///
/// A region is filled while it is writable and then sealed by [`protect`],
/// after which it can no longer be written through this API. This keeps the
/// copies sound even when part of the region becomes read-only.
///
/// [`protect`]: Region::protect
#[derive(Debug)]
pub struct Region
{
    base: NonNull<u8>,
    len: usize,
    sealed: bool,
}

// SAFETY: the region is exclusively owned by this value; the pointer is not
// tied to the creating thread.
unsafe impl Send for Region
{
}

// SAFETY: `&Region` only exposes the address and length.
unsafe impl Sync for Region
{
}

impl Region
{
    /// Maps `len` zero-filled, readable and writable bytes at exactly `addr`.
    ///
    /// Fails with `AddrInUse` if any part of the range is already mapped.
    pub fn
    reserve_at(addr: u64, len: usize) -> io::Result<Region>
    {
        let flags: libc::c_int = libc::MAP_PRIVATE | libc::MAP_ANON;
        let base: NonNull<u8> = map_exact(addr, len, Protection::READ_WRITE, flags, -1, 0)?;

        return (Ok(Region { base: base, len: len, sealed: false }));
    }

    /// Start address of the region.
    pub fn
    addr(&self) -> u64
    {
        return (self.base.as_ptr() as usize as u64);
    }

    /// Length of the region in bytes.
    pub fn
    len(&self) -> usize
    {
        return (self.len);
    }

    /// Always false: empty regions cannot be created.
    pub fn
    is_empty(&self) -> bool
    {
        return (self.len == 0);
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

    /// Copies bytes into the region. Fails once the region is sealed.
    pub fn
    write_at(&mut self, offset: usize, buf: &[u8]) -> io::Result<()>
    {
        if self.sealed
        {
            return (Err(io::Error::from(io::ErrorKind::PermissionDenied)));
        }
        self.check_range(offset, buf.len())?;

        // SAFETY: the range lies within the region, which is still mapped
        // read-write, and `buf` is a distinct Rust allocation.
        unsafe
        {
            ptr::copy_nonoverlapping(buf.as_ptr(), self.base.as_ptr().add(offset), buf.len());
        }

        return (Ok(()));
    }

    /// Changes the protection of a page-aligned part of the region and seals
    /// it against further writes through [`Region::write_at`].
    pub fn
    protect(&mut self, offset: usize, len: usize, prot: Protection) -> io::Result<()>
    {
        let page: usize = page_size();

        if offset % page != 0 || len % page != 0
        {
            return (Err(io::Error::from(io::ErrorKind::InvalidInput)));
        }
        self.check_range(offset, len)?;
        self.sealed = true;
        if len == 0
        {
            return (Ok(()));
        }

        // SAFETY: the range lies within the region this value owns, and no
        // Rust reference into it exists.
        let ret: libc::c_int = unsafe
        {
            libc::mprotect(self.base.as_ptr().add(offset).cast::<libc::c_void>(), len, prot.to_host())
        };

        if ret == -1
        {
            return (Err(io::Error::last_os_error()));
        }

        return (Ok(()));
    }
}

impl Drop for Region
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
