// SPDX-License-Identifier: MPL-2.0

//! `host_ops` glue: the host side of the ABI and the jump into the kernel.
//!
//! This is the only module of the runtime allowed to use `unsafe`. Every
//! entry validates what it receives and never unwinds: a Rust panic inside an
//! `extern "C"` function aborts the process.
//!
//! Entries not served yet are left null, so the kernel sees them as absent:
//! address spaces and execution arrive in M2, devices in M4.

use std::io::{self, Write};
use std::process;
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use liu_abi::status::{STATUS_INVALID, STATUS_IO, STATUS_OK};
use liu_abi::{provides, BootInfo, Error, HostOps, KernelEntry, KernelOps, Status};

use crate::cpu::{Cpu, Delivery};
use crate::image::KernelImage;
use crate::kmem::KernelMemory;

/// The single vCPU of this kernel process.
static CPU: Cpu = Cpu::new();

/// Memory of the running kernel, owned here while it runs.
static KMEM: Mutex<Option<KernelMemory>> = Mutex::new(None);

/// The kernel's table, filled by the kernel during `liu_kernel_entry`.
static KERNEL_OPS: AtomicPtr<KernelOps> = AtomicPtr::new(ptr::null_mut());

/// Exit status of the kernel process after `host_ops.panic`.
pub const EXIT_KERNEL_PANIC: i32 = 3;

/// Largest console write accepted in one call; longer writes are truncated.
const LOG_WRITE_MAX: usize = 1 << 20;

/// Builds a byte slice from a pointer received from the kernel.
///
/// # Safety
///
/// When `buf` is non-null it must be valid for reads of `len` bytes for the
/// lifetime `'a`.
unsafe fn
kernel_bytes<'a>(buf: *const u8, len: usize) -> &'a [u8]
{
    if buf.is_null() || len == 0
    {
        return (&[]);
    }

    // SAFETY: guaranteed by the caller.
    let bytes: &[u8] = unsafe { std::slice::from_raw_parts(buf, len.min(LOG_WRITE_MAX)) };

    return (bytes);
}

/// `host_ops.log_write`: early console output.
unsafe extern "C" fn
log_write(buf: *const u8, len: usize) -> ()
{
    // SAFETY: the ABI requires `buf` to point to `len` readable bytes.
    let bytes: &[u8] = unsafe { kernel_bytes(buf, len) };
    let mut out: io::StdoutLock = io::stdout().lock();

    let _: io::Result<()> = out.write_all(bytes);
    let _: io::Result<()> = out.flush();
}

/// `host_ops.panic`: the kernel cannot continue. Does not return.
unsafe extern "C" fn
panic(msg: *const u8, len: usize) -> ()
{
    // SAFETY: the ABI requires `msg` to point to `len` readable bytes.
    let bytes: &[u8] = unsafe { kernel_bytes(msg, len) };

    let _: io::Result<()> = io::stdout().flush();
    eprintln!("liu-kernel: kernel panic: {}", String::from_utf8_lossy(bytes));
    process::exit(EXIT_KERNEL_PANIC);
}

/// Runs the kernel's handlers for what is due. Entries the kernel did not
/// register are skipped, and a table with a bad header is ignored.
fn
dispatch(due: Delivery) -> ()
{
    let table: *mut KernelOps = KERNEL_OPS.load(Ordering::SeqCst);

    if table.is_null()
    {
        return;
    }

    // SAFETY: `KERNEL_OPS` points to the table `enter` passed to the
    // kernel, which outlives the kernel; it is only accessed through raw
    // pointers, and copying it does not create a lasting reference.
    let kernel: KernelOps = unsafe { ptr::read(table) };

    if kernel.header.check().is_err()
    {
        return;
    }
    if due.timer && provides!(kernel, KernelOps, timer)
    {
        if let Some(timer) = kernel.timer
        {
            // SAFETY: the kernel registered this entry point.
            unsafe
            {
                timer();
            }
        }
    }
    if due.irqs != 0 && provides!(kernel, KernelOps, irq)
    {
        if let Some(irq) = kernel.irq
        {
            // SAFETY: the kernel registered this entry point.
            unsafe
            {
                irq(due.irqs);
            }
        }
    }
}

/// A safe point: delivers what is due if interrupts are enabled.
fn
safe_point() -> ()
{
    CPU.deliver(&liu_platform::clock_monotonic_ns, &mut dispatch);
}

/// `host_ops.irq_enable`.
unsafe extern "C" fn
irq_enable() -> ()
{
    CPU.set_enabled(true);
    safe_point();
}

/// `host_ops.irq_disable`.
unsafe extern "C" fn
irq_disable() -> ()
{
    CPU.set_enabled(false);
}

/// `host_ops.irq_save`: disables interrupts; returns 1 if they were enabled.
unsafe extern "C" fn
irq_save() -> u64
{
    return (CPU.set_enabled(false) as u64);
}

/// `host_ops.irq_restore`: bit 0 of `flags` is the state to restore.
unsafe extern "C" fn
irq_restore(flags: u64) -> ()
{
    if flags & 1 != 0
    {
        CPU.set_enabled(true);
        safe_point();
    }
    else
    {
        CPU.set_enabled(false);
    }
}

/// `host_ops.idle`: blocks until an interrupt is due, then delivers it.
unsafe extern "C" fn
idle() -> ()
{
    CPU.wait(&liu_platform::clock_monotonic_ns);
    CPU.set_enabled(true);
    safe_point();
}

/// `host_ops.clock_monotonic`.
unsafe extern "C" fn
clock_monotonic() -> u64
{
    return (liu_platform::clock_monotonic_ns());
}

/// `host_ops.clock_realtime`.
unsafe extern "C" fn
clock_realtime() -> u64
{
    return (liu_platform::clock_realtime_ns());
}

/// `host_ops.timer_set`: arms the one-shot timer.
unsafe extern "C" fn
timer_set(deadline: u64) -> Status
{
    CPU.set_timer(deadline);
    return (STATUS_OK);
}

/// `host_ops.timer_cancel`.
unsafe extern "C" fn
timer_cancel() -> ()
{
    CPU.cancel_timer();
}

/// `host_ops.get_random`: fills `buf` with host entropy.
unsafe extern "C" fn
get_random(buf: *mut u8, len: usize) -> Status
{
    if len == 0
    {
        return (STATUS_OK);
    }
    if buf.is_null()
    {
        return (STATUS_INVALID);
    }

    // SAFETY: the ABI requires `buf` to point to `len` writable bytes that
    // nothing else accesses during the call.
    let bytes: &mut [u8] = unsafe { std::slice::from_raw_parts_mut(buf, len) };

    if liu_platform::fill_random(bytes).is_err()
    {
        return (STATUS_IO);
    }

    return (STATUS_OK);
}

/// Runs `op` on the running kernel's memory and converts the result.
fn
with_kmem(op: &mut dyn FnMut(&mut KernelMemory) -> Result<(), Error>) -> Status
{
    let mut guard: MutexGuard<'_, Option<KernelMemory>> = KMEM.lock().unwrap_or_else(PoisonError::into_inner);
    let result: Result<(), Error> = match guard.as_mut()
    {
        Some(kmem) => op(kmem),
        None => Err(Error::Invalid),
    };

    return (Error::from_result(result));
}

/// `host_ops.kmap`.
unsafe extern "C" fn
kmap(vaddr: u64, phys: u64, len: u64, prot: u32) -> Status
{
    return (with_kmem(&mut |kmem: &mut KernelMemory| -> Result<(), Error>
    {
        return (kmem.kmap(vaddr, phys, len, prot));
    }));
}

/// `host_ops.kunmap`.
unsafe extern "C" fn
kunmap(vaddr: u64, len: u64) -> Status
{
    return (with_kmem(&mut |kmem: &mut KernelMemory| -> Result<(), Error>
    {
        return (kmem.kunmap(vaddr, len));
    }));
}

/// `host_ops.kprotect`.
unsafe extern "C" fn
kprotect(vaddr: u64, len: u64, prot: u32) -> Status
{
    return (with_kmem(&mut |kmem: &mut KernelMemory| -> Result<(), Error>
    {
        return (kmem.kprotect(vaddr, len, prot));
    }));
}

/// `host_ops.kstack_alloc`: stores the lowest usable address in `out`.
unsafe extern "C" fn
kstack_alloc(size: u64, out: *mut u64) -> Status
{
    if out.is_null()
    {
        return (STATUS_INVALID);
    }

    let mut base: u64 = 0;
    let status: Status = with_kmem(&mut |kmem: &mut KernelMemory| -> Result<(), Error>
    {
        base = kmem.kstack_alloc(size)?;
        return (Ok(()));
    });

    if status == STATUS_OK
    {
        // SAFETY: checked non-null; the ABI requires `out` to be writable.
        unsafe
        {
            out.write(base);
        }
    }

    return (status);
}

/// `host_ops.kstack_free`.
unsafe extern "C" fn
kstack_free(addr: u64, size: u64) -> ()
{
    let _: Status = with_kmem(&mut |kmem: &mut KernelMemory| -> Result<(), Error>
    {
        kmem.kstack_free(addr, size);
        return (Ok(()));
    });
}

const fn
host_ops() -> HostOps
{
    let mut ops: HostOps = HostOps::empty();

    ops.log_write = Some(log_write);
    ops.panic = Some(panic);
    ops.kmap = Some(kmap);
    ops.kunmap = Some(kunmap);
    ops.kprotect = Some(kprotect);
    ops.kstack_alloc = Some(kstack_alloc);
    ops.kstack_free = Some(kstack_free);
    ops.irq_enable = Some(irq_enable);
    ops.irq_disable = Some(irq_disable);
    ops.irq_save = Some(irq_save);
    ops.irq_restore = Some(irq_restore);
    ops.idle = Some(idle);
    ops.clock_monotonic = Some(clock_monotonic);
    ops.clock_realtime = Some(clock_realtime);
    ops.timer_set = Some(timer_set);
    ops.timer_cancel = Some(timer_cancel);
    ops.get_random = Some(get_random);

    return (ops);
}

/// The table handed to the kernel. It lives for the whole process.
static HOST_OPS: HostOps = host_ops();

/// Calls `liu_kernel_entry` of a loaded image on the current thread, with
/// interrupts disabled and no timer armed. `kmem` serves the kernel memory
/// entries until the kernel returns, and is released then.
///
/// The call returns when the kernel shuts down; the result is its status.
/// One kernel runs per process: `enter` must not be called concurrently.
pub fn
enter(image: &KernelImage, kmem: KernelMemory, kernel: &mut KernelOps, boot: &BootInfo) -> Status
{
    let table: *mut KernelOps = kernel;

    *KMEM.lock().unwrap_or_else(PoisonError::into_inner) = Some(kmem);
    CPU.reset();
    KERNEL_OPS.store(table, Ordering::SeqCst);

    // SAFETY: `KernelImage` guarantees that `entry` lies in an executable
    // segment of an image that stays mapped while `image` is borrowed, and
    // the image was validated as an x86_64 kernel whose entry point is
    // `liu_kernel_entry`, which has the `KernelEntry` signature.
    let entry: KernelEntry =
        unsafe { std::mem::transmute::<usize, KernelEntry>(image.entry() as usize) };

    // SAFETY: the three tables are valid for the duration of the call:
    // `HOST_OPS` is static and the others are borrowed for it.
    let status: Status = unsafe { entry(&HOST_OPS, table, boot) };

    KERNEL_OPS.store(ptr::null_mut(), Ordering::SeqCst);
    CPU.reset();
    // Unmaps the window, physical memory and every stack.
    drop(KMEM.lock().unwrap_or_else(PoisonError::into_inner).take());

    return (status);
}
