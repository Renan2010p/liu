// SPDX-License-Identifier: MPL-2.0

//! LIU runtime: the kernel process.
//!
//! The runtime maps a freestanding kernel image at its link-time address,
//! creates guest physical memory and the `kmap` window after it (see
//! [`kmem`]), and calls `liu_kernel_entry` on a vCPU thread with the host
//! ABI tables (`docs/architecture.md` §5 and §6).

#![deny(unsafe_code)]

pub mod config;
pub mod cpu;
pub mod image;
pub mod kmem;
#[allow(unsafe_code)]
pub mod ops;
pub mod spaces;

use std::fmt;
use std::fs;
use std::io;
use std::thread;

use liu_abi::{BootInfo, KernelOps, Status, TableHeader, HANDLE_INVALID, PAGE_SIZE};
use liu_physmem::Physmem;
use liu_platform::{Capabilities, SharedMemory, TrapBackend, Window};

use kmem::{KernelMemory, KMAP_WINDOW_SIZE};
use spaces::Spaces;

pub use config::Config;
pub use image::{KernelImage, LoadError};

/// Stack size of the vCPU thread, which the kernel runs on at entry.
pub const VCPU_STACK_SIZE: usize = 8 << 20;

/// Alignment of guest physical memory and of the `kmap` window in the
/// kernel's address space.
pub const PHYSMEM_ALIGN: u64 = 2 << 20;

/// Why the runtime could not run the kernel.
#[derive(Debug)]
pub enum RuntimeError
{
    PageSize(usize),
    Read(io::Error),
    Load(LoadError),
    Physmem(u64, io::Error),
    Window(u64, io::Error),
    Probe(io::Error),
    Thread(io::Error),
    /// The vCPU thread ended without a status.
    VcpuPanicked,
}

impl fmt::Display for RuntimeError
{
    fn
    fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
        let result: fmt::Result = match self
        {
            RuntimeError::PageSize(size) => write!(f, "host page size {} is not supported", size),
            RuntimeError::Read(error) => write!(f, "cannot read the kernel image: {}", error),
            RuntimeError::Load(error) => write!(f, "cannot load the kernel image: {}", error),
            RuntimeError::Physmem(addr, error) =>
            {
                write!(f, "cannot create guest memory at {:#x}: {}", addr, error)
            }
            RuntimeError::Window(addr, error) =>
            {
                write!(f, "cannot reserve the kmap window at {:#x}: {}", addr, error)
            }
            RuntimeError::Probe(error) => write!(f, "cannot probe the host: {}", error),
            RuntimeError::Thread(error) => write!(f, "cannot start the vCPU thread: {}", error),
            RuntimeError::VcpuPanicked => f.write_str("the vCPU thread panicked"),
        };

        return (result);
    }
}

impl std::error::Error for RuntimeError
{
}

/// Returns the first aligned address after `end`. Physical memory follows
/// the image, as arch/um places it after `_end`, and the `kmap` window
/// follows physical memory.
pub fn
physmem_base(end: u64) -> u64
{
    return ((end + PHYSMEM_ALIGN - 1) & !(PHYSMEM_ALIGN - 1));
}

/// Loads the kernel described by `config`, runs it until it returns, and
/// returns the status it reported.
pub fn
run(config: &Config) -> Result<Status, RuntimeError>
{
    let page: usize = liu_platform::page_size();

    if page as u64 != PAGE_SIZE
    {
        return (Err(RuntimeError::PageSize(page)));
    }

    // Probing forks helper processes: do it before any thread exists.
    let caps: Capabilities = liu_platform::probe().map_err(RuntimeError::Probe)?;
    let backend: Option<Box<dyn TrapBackend>> = liu_platform::open_trap_backend(&caps).ok();
    let data: Vec<u8> = fs::read(&config.kernel).map_err(RuntimeError::Read)?;
    let image: KernelImage = KernelImage::load(&data).map_err(RuntimeError::Load)?;

    drop(data);

    let base: u64 = physmem_base(image.base() + image.size());
    let physmem: Physmem = Physmem::create_at(config.memory, base)
        .map_err(|error: io::Error| -> RuntimeError { return (RuntimeError::Physmem(base, error)); })?;
    let window_base: u64 = physmem_base(physmem.base() + physmem.size());
    let window: Window = Window::reserve_at(window_base, KMAP_WINDOW_SIZE as usize)
        .map_err(|error: io::Error| -> RuntimeError { return (RuntimeError::Window(window_base, error)); })?;
    let guest_physmem: SharedMemory = physmem.shm().try_clone()
        .map_err(|error: io::Error| -> RuntimeError { return (RuntimeError::Physmem(base, error)); })?;
    let spaces: Spaces = Spaces::new(backend, guest_physmem);

    eprintln!("liu-kernel: trap backend: {}", spaces.backend_name());
    let kmem: KernelMemory = KernelMemory::new(physmem, window);
    let cmdline: &[u8] = config.cmdline.as_bytes();

    let status: Result<Status, RuntimeError> = thread::scope(
        |scope: &thread::Scope| -> Result<Status, RuntimeError>
        {
            let handle: thread::ScopedJoinHandle<'_, Status> = thread::Builder::new()
                .name(String::from("vcpu"))
                .stack_size(VCPU_STACK_SIZE)
                .spawn_scoped(scope, || -> Status
                {
                    return (vcpu_main(&image, kmem, spaces, cmdline));
                })
                .map_err(RuntimeError::Thread)?;

            return (handle.join().map_err(|_: Box<dyn std::any::Any + Send>| -> RuntimeError
            {
                return (RuntimeError::VcpuPanicked);
            }));
        },
    );

    return (status);
}

/// Body of the vCPU thread: builds the boot tables and enters the kernel,
/// which owns `kmem` until it returns.
fn
vcpu_main(image: &KernelImage, kmem: KernelMemory, spaces: Spaces, cmdline: &[u8]) -> Status
{
    let boot: BootInfo = BootInfo
    {
        header: TableHeader::new(size_of::<BootInfo>()),
        physmem_base: kmem.physmem().base(),
        physmem_size: kmem.physmem().size(),
        // Handles arrive with devices (M4).
        physmem_handle: HANDLE_INVALID,
        image_base: image.base(),
        image_size: image.size(),
        cmdline: cmdline.as_ptr(),
        cmdline_len: cmdline.len() as u64,
        initrd_offset: 0,
        initrd_size: 0,
        // Guest FPU state arrives with guest spaces (M2).
        fpu_state_size: 0,
        page_size: PAGE_SIZE,
        kmap_base: kmem.window_base(),
        kmap_size: kmem.window_size(),
    };
    let mut kernel: KernelOps = KernelOps::empty();

    return (ops::enter(image, kmem, spaces, &mut kernel, &boot));
}

#[cfg(test)]
mod tests
{
    use super::*;

    #[test]
    fn
    physmem_follows_the_image_aligned() -> ()
    {
        assert_eq!(physmem_base(0x6000_5000), 0x6020_0000);
        assert_eq!(physmem_base(0x6020_0000), 0x6020_0000);
    }
}
