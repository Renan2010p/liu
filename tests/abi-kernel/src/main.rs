// SPDX-License-Identifier: MPL-2.0

//! LIU ABI test kernel.
//!
//! A freestanding image that plays the role of the Linux kernel: the runtime
//! loads it exactly as it will load `linux.liu`, calls `liu_kernel_entry`,
//! and serves the host ABI. Each check prints one `abi-kernel:` line, and the
//! last line is `abi-kernel: pass` when everything succeeded, which is what
//! `cargo xtask test-abi` looks for.
//!
//! This crate is kernel-side code, so like the C in `kernel/` it works with
//! raw pointers received across the boundary.

#![no_std]
#![no_main]

use core::fmt::{self, Write};
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicPtr, Ordering};

use liu_abi::status::{STATUS_INVALID, STATUS_OK, STATUS_UNSUPPORTED};
use liu_abi::{provides, BootInfo, HostOps, KernelOps, Status, PAGE_SIZE};

/// The host table, kept for the panic handler.
static HOST: AtomicPtr<HostOps> = AtomicPtr::new(core::ptr::null_mut());

/// Console that writes through `host_ops.log_write`.
struct Console<'a>
{
    host: &'a HostOps,
}

impl Write for Console<'_>
{
    fn
    write_str(&mut self, text: &str) -> fmt::Result
    {
        if let Some(log_write) = self.host.log_write
        {
            // SAFETY: `text` is valid for reads of `text.len()` bytes for the
            // duration of the call.
            unsafe
            {
                log_write(text.as_ptr(), text.len());
            }
        }

        return (Ok(()));
    }
}

/// Runs every check and returns the status the entry point reports.
fn
run(host: &HostOps, kernel: &mut KernelOps, boot: &BootInfo) -> Status
{
    let mut con: Console = Console { host: host };

    let _: fmt::Result = writeln!(con, "abi-kernel: hello from the LIU ABI test kernel");

    if boot.header.check().is_err()
    {
        let _: fmt::Result = writeln!(con, "abi-kernel: FAIL boot info header");
        return (STATUS_INVALID);
    }
    if boot.page_size != PAGE_SIZE
    {
        let _: fmt::Result = writeln!(con, "abi-kernel: FAIL page size {}", boot.page_size);
        return (STATUS_INVALID);
    }

    let entry: u64 = liu_kernel_entry as *const () as usize as u64;

    if entry < boot.image_base || entry >= boot.image_base + boot.image_size
    {
        let _: fmt::Result = writeln!(con, "abi-kernel: FAIL entry {:#x} outside the image", entry);
        return (STATUS_INVALID);
    }
    let _: fmt::Result = writeln!(con, "abi-kernel: image {:#x} + {:#x}", boot.image_base, boot.image_size);
    let _: fmt::Result = writeln!(
        con,
        "abi-kernel: physmem {} MiB at {:#x}",
        boot.physmem_size >> 20,
        boot.physmem_base
    );

    let cmdline: &[u8] = if boot.cmdline.is_null() || boot.cmdline_len == 0
    {
        &[]
    }
    else
    {
        // SAFETY: the runtime guarantees `cmdline` points to `cmdline_len`
        // bytes that live as long as the kernel.
        unsafe { core::slice::from_raw_parts(boot.cmdline, boot.cmdline_len as usize) }
    };
    let text: &str = core::str::from_utf8(cmdline).unwrap_or("<not utf-8>");

    let _: fmt::Result = writeln!(con, "abi-kernel: cmdline \"{}\"", text);

    *kernel = KernelOps::empty();
    let _: fmt::Result = writeln!(con, "abi-kernel: pass");

    return (STATUS_OK);
}

/// Entry point of the image, called by the runtime on the vCPU thread.
///
/// # Safety
///
/// `host`, `kernel` and `boot` must be valid tables that outlive the call.
#[no_mangle]
pub unsafe extern "C" fn
liu_kernel_entry(host: *const HostOps, kernel: *mut KernelOps, boot: *const BootInfo) -> Status
{
    if host.is_null() || kernel.is_null() || boot.is_null()
    {
        return (STATUS_INVALID);
    }

    // SAFETY: checked non-null; the caller guarantees validity.
    let host_ref: &HostOps = unsafe { &*host };

    if host_ref.header.check().is_err()
    {
        return (STATUS_INVALID);
    }
    if !provides!(host_ref, HostOps, log_write)
    {
        return (STATUS_UNSUPPORTED);
    }
    HOST.store(host.cast_mut(), Ordering::Relaxed);

    // SAFETY: checked non-null; the caller guarantees validity and that no
    // other reference to these tables exists during the call.
    let status: Status = unsafe { run(host_ref, &mut *kernel, &*boot) };

    return (status);
}

#[panic_handler]
fn
panic(info: &PanicInfo) -> !
{
    let host: *mut HostOps = HOST.load(Ordering::Relaxed);

    if !host.is_null()
    {
        // SAFETY: `HOST` is only set to the table the runtime passed in,
        // which outlives the kernel.
        let host_ref: &HostOps = unsafe { &*host };
        let mut con: Console = Console { host: host_ref };
        let _: fmt::Result = writeln!(con, "abi-kernel: FAIL {}", info);

        if let Some(host_panic) = host_ref.panic
        {
            let msg: &str = "abi-kernel panicked";

            // SAFETY: `msg` is a valid static string.
            unsafe
            {
                host_panic(msg.as_ptr(), msg.len());
            }
        }
    }

    loop
    {
        core::hint::spin_loop();
    }
}
