// SPDX-License-Identifier: MPL-2.0

//! Kernel memory: the `kmap` window and kernel thread stacks.
//!
//! The kernel process address space is laid out as
//!
//! ```text
//! 0x60000000  kernel image (link address, like arch/um)
//!             physical memory, linear map      (2 MiB aligned)
//!             kmap window, PROT_NONE when idle (2 MiB aligned, KMAP_WINDOW_SIZE)
//! ```
//!
//! `kmap` only ever maps inside the window, so whatever the kernel asks, it
//! cannot replace the runtime's own memory or the image. Every argument
//! comes from the kernel and is validated here before reaching the host.

use std::collections::HashMap;
use std::io;

use liu_abi::{Error, PAGE_SIZE, PROT_EXEC, PROT_MASK, PROT_READ, PROT_WRITE};
use liu_physmem::{PhysError, Physmem};
use liu_platform::{Protection, Stack, Window};

/// Size of the `kmap` window.
pub const KMAP_WINDOW_SIZE: u64 = 1 << 30;

/// Largest stack `kstack_alloc` hands out.
pub const KSTACK_MAX: u64 = 16 << 20;

/// Most stacks alive at once.
pub const KSTACK_COUNT_MAX: usize = 4096;

const BITS: u64 = 64;

/// Converts ABI protection bits, refusing unknown bits and W+X.
fn
protection(prot: u32) -> Result<Protection, Error>
{
    if prot & !PROT_MASK != 0
    {
        return (Err(Error::Invalid));
    }
    if prot & PROT_WRITE != 0 && prot & PROT_EXEC != 0
    {
        return (Err(Error::Denied));
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

fn
host_error(error: io::Error) -> Error
{
    if error.kind() == io::ErrorKind::OutOfMemory
    {
        return (Error::NoMemory);
    }

    return (Error::Io);
}

/// Physical memory, the `kmap` window and the stacks of one kernel.
#[derive(Debug)]
pub struct KernelMemory
{
    physmem: Physmem,
    window: Window,
    /// One bit per window page: set when the page is mapped.
    mapped: Vec<u64>,
    stacks: HashMap<u64, Stack>,
}

impl KernelMemory
{
    /// Takes ownership of physical memory and an empty window.
    pub fn
    new(physmem: Physmem, window: Window) -> KernelMemory
    {
        let pages: u64 = window.len() as u64 / PAGE_SIZE;
        let words: usize = pages.div_ceil(BITS) as usize;
        let kmem: KernelMemory = KernelMemory
        {
            physmem: physmem,
            window: window,
            mapped: vec![0; words],
            stacks: HashMap::new(),
        };

        return (kmem);
    }

    /// Guest physical memory.
    pub fn
    physmem(&self) -> &Physmem
    {
        return (&self.physmem);
    }

    /// Start of the `kmap` window.
    pub fn
    window_base(&self) -> u64
    {
        return (self.window.addr());
    }

    /// Size of the `kmap` window.
    pub fn
    window_size(&self) -> u64
    {
        return (self.window.len() as u64);
    }

    /// Validates a window range and returns its offset in the window.
    fn
    window_offset(&self, vaddr: u64, len: u64) -> Result<u64, Error>
    {
        if len == 0 || vaddr % PAGE_SIZE != 0 || len % PAGE_SIZE != 0
        {
            return (Err(Error::Invalid));
        }

        let base: u64 = self.window_base();
        let end: Option<u64> = vaddr.checked_add(len);

        if vaddr < base || end.is_none() || end.unwrap_or(u64::MAX) > base + self.window_size()
        {
            return (Err(Error::Fault));
        }

        return (Ok(vaddr - base));
    }

    fn
    set_mapped(&mut self, offset: u64, len: u64, on: bool) -> ()
    {
        let first: u64 = offset / PAGE_SIZE;

        for page in first..first + len / PAGE_SIZE
        {
            let bit: u64 = 1 << (page % BITS);
            let word: &mut u64 = &mut self.mapped[(page / BITS) as usize];

            if on
            {
                *word |= bit;
            }
            else
            {
                *word &= !bit;
            }
        }
    }

    fn
    all_mapped(&self, offset: u64, len: u64) -> bool
    {
        let first: u64 = offset / PAGE_SIZE;

        return ((first..first + len / PAGE_SIZE).all(|page: u64| -> bool
        {
            return (self.mapped[(page / BITS) as usize] & (1 << (page % BITS)) != 0);
        }));
    }

    /// `kmap`: maps physical memory into the window.
    pub fn
    kmap(&mut self, vaddr: u64, phys: u64, len: u64, prot: u32) -> Result<(), Error>
    {
        let offset: u64 = self.window_offset(vaddr, len)?;
        let prot: Protection = protection(prot)?;

        if phys % PAGE_SIZE != 0
        {
            return (Err(Error::Invalid));
        }
        self.physmem.check(phys, len).map_err(|error: PhysError| -> Error
        {
            return (match error
            {
                PhysError::Invalid => Error::Invalid,
                PhysError::OutOfRange => Error::Fault,
            });
        })?;
        self.window
            .map_shared(offset as usize, self.physmem.shm(), phys, len as usize, prot)
            .map_err(host_error)?;
        self.set_mapped(offset, len, true);

        return (Ok(()));
    }

    /// `kunmap`: makes a window range inaccessible again.
    pub fn
    kunmap(&mut self, vaddr: u64, len: u64) -> Result<(), Error>
    {
        let offset: u64 = self.window_offset(vaddr, len)?;

        self.window.unmap(offset as usize, len as usize).map_err(host_error)?;
        self.set_mapped(offset, len, false);

        return (Ok(()));
    }

    /// `kprotect`: changes the protection of mapped window pages.
    pub fn
    kprotect(&mut self, vaddr: u64, len: u64, prot: u32) -> Result<(), Error>
    {
        let offset: u64 = self.window_offset(vaddr, len)?;
        let prot: Protection = protection(prot)?;

        if !self.all_mapped(offset, len)
        {
            return (Err(Error::Fault));
        }
        self.window.protect(offset as usize, len as usize, prot).map_err(host_error)?;

        return (Ok(()));
    }

    /// `kstack_alloc`: returns the lowest usable address of a new stack.
    pub fn
    kstack_alloc(&mut self, size: u64) -> Result<u64, Error>
    {
        if size == 0 || size % PAGE_SIZE != 0 || size > KSTACK_MAX
        {
            return (Err(Error::Invalid));
        }
        if self.stacks.len() >= KSTACK_COUNT_MAX
        {
            return (Err(Error::NoMemory));
        }

        let stack: Stack = Stack::alloc(size as usize).map_err(host_error)?;
        let base: u64 = stack.base();

        self.stacks.insert(base, stack);
        return (Ok(base));
    }

    /// `kstack_free`: frees a stack given its address and size. Anything
    /// that does not match a live stack is ignored.
    pub fn
    kstack_free(&mut self, addr: u64, size: u64) -> ()
    {
        let matches: bool = match self.stacks.get(&addr)
        {
            Some(stack) => stack.size() as u64 == size,
            None => false,
        };

        if matches
        {
            self.stacks.remove(&addr);
        }
    }

    /// Number of live stacks.
    pub fn
    stack_count(&self) -> usize
    {
        return (self.stacks.len());
    }
}

#[cfg(test)]
mod tests
{
    use super::*;

    const PHYS: u64 = 1 << 20;
    const WINDOW: usize = 1 << 20;

    /// Builds kernel memory with a window at a free address, retrying when
    /// a test running in parallel takes the address first.
    fn
    kmem() -> KernelMemory
    {
        for _ in 0..100
        {
            let probe: Physmem = Physmem::create(WINDOW as u64).unwrap();
            let addr: u64 = probe.base();

            drop(probe);
            if let Ok(window) = Window::reserve_at(addr, WINDOW)
            {
                return (KernelMemory::new(Physmem::create(PHYS).unwrap(), window));
            }
        }
        panic!("no free address for the window");
    }

    #[test]
    fn
    validates_protection() -> ()
    {
        assert_eq!(protection(PROT_READ | PROT_EXEC), Ok(Protection::READ.union(Protection::EXEC)));
        assert_eq!(protection(PROT_WRITE | PROT_EXEC), Err(Error::Denied));
        assert_eq!(protection(0x80), Err(Error::Invalid));
    }

    #[test]
    fn
    maps_physical_pages_into_the_window() -> ()
    {
        let mut k: KernelMemory = kmem();
        let base: u64 = k.window_base();

        assert_eq!(k.kmap(base, 0x2000, 0x2000, PROT_READ | PROT_WRITE), Ok(()));
        assert_eq!(k.kprotect(base, 0x2000, PROT_READ), Ok(()));
        assert_eq!(k.kunmap(base, 0x1000), Ok(()));
        assert_eq!(k.kprotect(base, 0x2000, PROT_READ), Err(Error::Fault));
        assert_eq!(k.kprotect(base + 0x1000, 0x1000, PROT_READ), Ok(()));
    }

    #[test]
    fn
    refuses_bad_kmap_arguments() -> ()
    {
        let mut k: KernelMemory = kmem();
        let base: u64 = k.window_base();
        let end: u64 = base + k.window_size();
        let rw: u32 = PROT_READ | PROT_WRITE;

        assert_eq!(k.kmap(base + 1, 0, 0x1000, rw), Err(Error::Invalid));
        assert_eq!(k.kmap(base, 1, 0x1000, rw), Err(Error::Invalid));
        assert_eq!(k.kmap(base, 0, 0, rw), Err(Error::Invalid));
        assert_eq!(k.kmap(base - 0x1000, 0, 0x1000, rw), Err(Error::Fault));
        assert_eq!(k.kmap(end - 0x1000, 0, 0x2000, rw), Err(Error::Fault));
        assert_eq!(k.kmap(base, PHYS - 0x1000, 0x2000, rw), Err(Error::Fault));
        assert_eq!(k.kmap(base, 0, 0x1000, PROT_WRITE | PROT_EXEC), Err(Error::Denied));
        assert_eq!(k.kunmap(!0xfff, 0x2000), Err(Error::Fault));
    }

    #[test]
    fn
    allocates_and_frees_stacks() -> ()
    {
        let mut k: KernelMemory = kmem();
        let stack: u64 = k.kstack_alloc(0x4000).unwrap();

        assert_eq!(stack % PAGE_SIZE, 0);
        assert_eq!(k.kstack_alloc(0), Err(Error::Invalid));
        assert_eq!(k.kstack_alloc(0x1001), Err(Error::Invalid));
        assert_eq!(k.kstack_alloc(KSTACK_MAX + PAGE_SIZE), Err(Error::Invalid));
        k.kstack_free(stack, 0x1000);
        assert_eq!(k.stack_count(), 1);
        k.kstack_free(stack, 0x4000);
        assert_eq!(k.stack_count(), 0);
    }
}
