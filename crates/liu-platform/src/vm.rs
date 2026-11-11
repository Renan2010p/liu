// SPDX-License-Identifier: MPL-2.0

//! Memory placed at fixed addresses: the kernel image and guest physical
//! memory must appear exactly where the kernel was linked to find them.

use std::io;
use std::os::fd::{AsFd, AsRawFd};
use std::ptr::{self, NonNull};

use crate::host;
use crate::shm::{Protection, SharedMemory};

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

/// Checks that `[offset, offset + len)` is a non-empty, page-aligned range
/// inside a mapping of `total` bytes.
fn
check_pages(offset: usize, len: usize, total: usize) -> io::Result<()>
{
    let page: usize = page_size();
    let end: Option<usize> = offset.checked_add(len);

    if len == 0 || offset % page != 0 || len % page != 0 || end.is_none() || end.unwrap_or(usize::MAX) > total
    {
        return (Err(io::Error::from(io::ErrorKind::InvalidInput)));
    }

    return (Ok(()));
}

/// A range of address space at a fixed address, reserved inaccessible, in
/// which shared memory can be mapped and unmapped page by page.
///
/// The window owns its whole range, so mappings inside it replace each
/// other without ever touching memory outside it. No Rust reference into
/// the window is ever created: it is used only by the kernel image.
#[derive(Debug)]
pub struct Window
{
    base: NonNull<u8>,
    len: usize,
}

// SAFETY: the window is exclusively owned by this value; the pointer is not
// tied to the creating thread.
unsafe impl Send for Window
{
}

impl Window
{
    /// Reserves `len` inaccessible bytes at exactly `addr`.
    ///
    /// Fails with `AddrInUse` if any part of the range is already mapped.
    pub fn
    reserve_at(addr: u64, len: usize) -> io::Result<Window>
    {
        let flags: libc::c_int = libc::MAP_PRIVATE | libc::MAP_ANON;
        let base: NonNull<u8> = map_exact(addr, len, Protection::NONE, flags, -1, 0)?;

        return (Ok(Window { base: base, len: len }));
    }

    /// Start address of the window.
    pub fn
    addr(&self) -> u64
    {
        return (self.base.as_ptr() as usize as u64);
    }

    /// Length of the window in bytes.
    pub fn
    len(&self) -> usize
    {
        return (self.len);
    }

    /// Always false: empty windows cannot be created.
    pub fn
    is_empty(&self) -> bool
    {
        return (self.len == 0);
    }

    /// Replaces `[offset, offset + len)` of the window by a mapping of
    /// `shm` starting at `shm_offset`.
    fn
    replace(
        &mut self,
        offset: usize,
        len: usize,
        prot: Protection,
        flags: libc::c_int,
        fd: libc::c_int,
        shm_offset: libc::off_t,
    ) -> io::Result<()>
    {
        check_pages(offset, len, self.len)?;

        // SAFETY: the range lies inside the window this value owns, so
        // MAP_FIXED replaces only window pages, which no Rust reference
        // points into.
        let got: *mut libc::c_void = unsafe
        {
            libc::mmap(
                self.base.as_ptr().add(offset).cast::<libc::c_void>(),
                len,
                prot.to_host(),
                flags | libc::MAP_FIXED,
                fd,
                shm_offset,
            )
        };

        if got == libc::MAP_FAILED
        {
            return (Err(io::Error::last_os_error()));
        }

        return (Ok(()));
    }

    /// Maps `len` bytes of `shm` from `shm_offset` at `offset` in the window,
    /// replacing whatever was there.
    pub fn
    map_shared(
        &mut self,
        offset: usize,
        shm: &SharedMemory,
        shm_offset: u64,
        len: usize,
        prot: Protection,
    ) -> io::Result<()>
    {
        let end: Option<u64> = shm_offset.checked_add(len as u64);

        if end.is_none() || end.unwrap_or(u64::MAX) > shm.size() || shm_offset % page_size() as u64 != 0
        {
            return (Err(io::Error::from(io::ErrorKind::InvalidInput)));
        }

        let host_offset: libc::off_t = match libc::off_t::try_from(shm_offset)
        {
            Ok(host_offset) => host_offset,
            Err(_) =>
            {
                return (Err(io::Error::from(io::ErrorKind::InvalidInput)));
            }
        };

        return (self.replace(offset, len, prot, libc::MAP_SHARED, shm.as_fd().as_raw_fd(), host_offset));
    }

    /// Returns `[offset, offset + len)` of the window to the reserved,
    /// inaccessible state.
    pub fn
    unmap(&mut self, offset: usize, len: usize) -> io::Result<()>
    {
        return (self.replace(offset, len, Protection::NONE, libc::MAP_PRIVATE | libc::MAP_ANON, -1, 0));
    }

    /// Changes the protection of `[offset, offset + len)` of the window.
    pub fn
    protect(&mut self, offset: usize, len: usize, prot: Protection) -> io::Result<()>
    {
        check_pages(offset, len, self.len)?;

        // SAFETY: the range lies inside the window this value owns, which no
        // Rust reference points into.
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

impl Drop for Window
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

/// A stack at a host-chosen address with an inaccessible guard page below
/// it, so that an overflow faults instead of corrupting memory.
#[derive(Debug)]
pub struct Stack
{
    /// Start of the whole mapping, guard page included.
    mapping: NonNull<u8>,
    /// Size of the usable part.
    size: usize,
}

// SAFETY: the stack is exclusively owned by this value; the pointer is not
// tied to the creating thread.
unsafe impl Send for Stack
{
}

impl Stack
{
    /// Allocates a zero-filled stack of `size` usable bytes, a non-zero
    /// multiple of the page size.
    pub fn
    alloc(size: usize) -> io::Result<Stack>
    {
        let page: usize = page_size();
        let total: usize = size.checked_add(page).ok_or(io::Error::from(io::ErrorKind::InvalidInput))?;

        if size == 0 || size % page != 0
        {
            return (Err(io::Error::from(io::ErrorKind::InvalidInput)));
        }

        // SAFETY: a fresh mapping at a host-chosen address cannot alias any
        // existing Rust object.
        let addr: *mut libc::c_void = unsafe
        {
            libc::mmap(
                std::ptr::null_mut(),
                total,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANON,
                -1,
                0,
            )
        };

        if addr == libc::MAP_FAILED
        {
            return (Err(io::Error::last_os_error()));
        }

        let mapping: NonNull<u8> = match NonNull::new(addr.cast::<u8>())
        {
            Some(mapping) => mapping,
            None =>
            {
                return (Err(io::Error::other("mmap returned a null mapping")));
            }
        };
        let stack: Stack = Stack { mapping: mapping, size: size };

        // SAFETY: the guard page is the first page of the mapping just
        // created, which nothing references yet.
        let ret: libc::c_int = unsafe { libc::mprotect(addr, page, libc::PROT_NONE) };

        if ret == -1
        {
            return (Err(io::Error::last_os_error()));
        }

        return (Ok(stack));
    }

    /// Lowest usable address, just above the guard page.
    pub fn
    base(&self) -> u64
    {
        return (self.mapping.as_ptr() as usize as u64 + page_size() as u64);
    }

    /// Size of the usable part in bytes.
    pub fn
    size(&self) -> usize
    {
        return (self.size);
    }

    /// Address of the guard page.
    pub fn
    guard(&self) -> u64
    {
        return (self.mapping.as_ptr() as usize as u64);
    }
}

impl Drop for Stack
{
    fn
    drop(&mut self) -> ()
    {
        // SAFETY: `mapping` and `size` plus the guard page describe a
        // mapping this value owns.
        unsafe
        {
            libc::munmap(self.mapping.as_ptr().cast::<libc::c_void>(), self.size + page_size());
        }
    }
}
