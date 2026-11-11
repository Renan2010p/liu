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

use liu_abi::{BootInfo, HostOps, KernelEntry, KernelOps, Status};

use crate::image::KernelImage;

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

const fn
host_ops() -> HostOps
{
    let mut ops: HostOps = HostOps::empty();

    ops.log_write = Some(log_write);
    ops.panic = Some(panic);

    return (ops);
}

/// The table handed to the kernel. It lives for the whole process.
static HOST_OPS: HostOps = host_ops();

/// Calls `liu_kernel_entry` of a loaded image on the current thread.
///
/// The call returns when the kernel shuts down; the result is its status.
pub fn
enter(image: &KernelImage, kernel: &mut KernelOps, boot: &BootInfo) -> Status
{
    // SAFETY: `KernelImage` guarantees that `entry` lies in an executable
    // segment of an image that stays mapped while `image` is borrowed, and
    // the image was validated as an x86_64 kernel whose entry point is
    // `liu_kernel_entry`, which has the `KernelEntry` signature.
    let entry: KernelEntry =
        unsafe { std::mem::transmute::<usize, KernelEntry>(image.entry() as usize) };

    // SAFETY: the three tables are valid for the duration of the call:
    // `HOST_OPS` is static and the others are borrowed for it.
    let status: Status = unsafe { entry(&HOST_OPS, kernel, boot) };

    return (status);
}
