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
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};

use liu_abi::status::{STATUS_DENIED, STATUS_FAULT, STATUS_INVALID, STATUS_OK, STATUS_UNSUPPORTED};
use liu_abi::{provides, BootInfo, HostOps, KernelOps, Status, PAGE_SIZE, PROT_EXEC, PROT_READ, PROT_WRITE};

/// The host table, kept for the panic and timer handlers.
static HOST: AtomicPtr<HostOps> = AtomicPtr::new(core::ptr::null_mut());

/// Number of `kernel_ops.timer` calls.
static TICKS: AtomicU32 = AtomicU32::new(0);

/// Set when a handler ran with interrupts enabled.
static HANDLER_ENABLED: AtomicBool = AtomicBool::new(false);

/// 2020-01-01T00:00:00Z in nanoseconds.
const YEAR_2020_NS: u64 = 1_577_836_800 * 1_000_000_000;

/// Delay of the idle test, in nanoseconds.
const IDLE_DELAY_NS: u64 = 10_000_000;

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

/// A host entry the test needs is missing.
struct Missing(&'static str);

/// One conformance check: `Ok(true)` passed, `Ok(false)` failed.
type Check = fn(&mut Console, &HostOps, &BootInfo) -> Result<bool, Missing>;

/// Returns a host entry or the name of the missing one.
fn
need<T>(entry: Option<T>, name: &'static str) -> Result<T, Missing>
{
    return (entry.ok_or(Missing(name)));
}

/// `kernel_ops.timer`: counts ticks and records the interrupt state.
unsafe extern "C" fn
on_timer() -> ()
{
    let host: *mut HostOps = HOST.load(Ordering::Relaxed);

    // SAFETY: `HOST` is set before interrupts can be enabled and points to
    // the runtime's static table.
    if let Some(irq_save) = unsafe { (*host).irq_save }
    {
        // SAFETY: a host entry with no arguments. Saving does not change
        // the state inside a handler, where interrupts must be disabled.
        let flags: u64 = unsafe { irq_save() };

        if flags != 0
        {
            HANDLER_ENABLED.store(true, Ordering::Relaxed);
        }
    }
    TICKS.fetch_add(1, Ordering::Relaxed);
}

/// Boot information and the command line.
fn
check_boot(con: &mut Console, boot: &BootInfo) -> bool
{
    if boot.header.check().is_err()
    {
        let _: fmt::Result = writeln!(con, "abi-kernel: FAIL boot info header");
        return (false);
    }
    if boot.page_size != PAGE_SIZE
    {
        let _: fmt::Result = writeln!(con, "abi-kernel: FAIL page size {}", boot.page_size);
        return (false);
    }

    let entry: u64 = liu_kernel_entry as *const () as usize as u64;

    if entry < boot.image_base || entry >= boot.image_base + boot.image_size
    {
        let _: fmt::Result = writeln!(con, "abi-kernel: FAIL entry {:#x} outside the image", entry);
        return (false);
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
    return (true);
}

/// `clock_monotonic` and `clock_realtime`.
fn
check_clocks(con: &mut Console, host: &HostOps, _boot: &BootInfo) -> Result<bool, Missing>
{
    let monotonic: unsafe extern "C" fn() -> u64 = need(host.clock_monotonic, "clock_monotonic")?;
    let realtime: unsafe extern "C" fn() -> u64 = need(host.clock_realtime, "clock_realtime")?;

    // SAFETY: host entries with no arguments.
    let (first, second, wall): (u64, u64, u64) = unsafe { (monotonic(), monotonic(), realtime()) };

    if second < first || wall < YEAR_2020_NS
    {
        let _: fmt::Result = writeln!(con, "abi-kernel: FAIL clocks {} {} {}", first, second, wall);
        return (Ok(false));
    }
    let _: fmt::Result = writeln!(con, "abi-kernel: clocks ok (realtime {} s)", wall / 1_000_000_000);
    return (Ok(true));
}

/// `get_random`.
fn
check_random(con: &mut Console, host: &HostOps, _boot: &BootInfo) -> Result<bool, Missing>
{
    let random: unsafe extern "C" fn(*mut u8, usize) -> Status = need(host.get_random, "get_random")?;
    let mut a: [u8; 64] = [0; 64];
    let mut b: [u8; 64] = [0; 64];

    // SAFETY: both buffers are valid for writes of their length; the null
    // buffer must be refused without being written.
    let status: (Status, Status, Status) = unsafe
    {
        (random(a.as_mut_ptr(), a.len()), random(b.as_mut_ptr(), b.len()), random(core::ptr::null_mut(), 8))
    };

    if status != (STATUS_OK, STATUS_OK, STATUS_INVALID) || a == b || a == [0; 64]
    {
        let _: fmt::Result = writeln!(con, "abi-kernel: FAIL get_random {:?}", status);
        return (Ok(false));
    }
    let _: fmt::Result = writeln!(con, "abi-kernel: random ok");
    return (Ok(true));
}

/// Interrupt flags, the one-shot timer, `idle` and delivery at safe points.
fn
check_timer(con: &mut Console, host: &HostOps, _boot: &BootInfo) -> Result<bool, Missing>
{
    let now: unsafe extern "C" fn() -> u64 = need(host.clock_monotonic, "clock_monotonic")?;
    let enable: unsafe extern "C" fn() -> () = need(host.irq_enable, "irq_enable")?;
    let disable: unsafe extern "C" fn() -> () = need(host.irq_disable, "irq_disable")?;
    let save: unsafe extern "C" fn() -> u64 = need(host.irq_save, "irq_save")?;
    let restore: unsafe extern "C" fn(u64) -> () = need(host.irq_restore, "irq_restore")?;
    let idle: unsafe extern "C" fn() -> () = need(host.idle, "idle")?;
    let set: unsafe extern "C" fn(u64) -> Status = need(host.timer_set, "timer_set")?;
    let cancel: unsafe extern "C" fn() -> () = need(host.timer_cancel, "timer_cancel")?;

    // SAFETY: every call below is a host entry taking plain integers, made
    // from the vCPU thread the kernel was entered on.
    unsafe
    {
        // Interrupts start disabled.
        let at_entry: u64 = save();

        restore(1);
        let enabled: u64 = save();

        restore(0);
        if at_entry != 0 || enabled != 1
        {
            let _: fmt::Result = writeln!(con, "abi-kernel: FAIL irq flags {} {}", at_entry, enabled);
            return (Ok(false));
        }

        // idle() sleeps until the deadline, then the handler runs.
        let start: u64 = now();

        enable();
        if set(start + IDLE_DELAY_NS) != STATUS_OK
        {
            let _: fmt::Result = writeln!(con, "abi-kernel: FAIL timer_set");
            return (Ok(false));
        }
        idle();

        let slept: u64 = now() - start;

        if TICKS.load(Ordering::Relaxed) != 1 || slept < IDLE_DELAY_NS
        {
            let _: fmt::Result = writeln!(
                con,
                "abi-kernel: FAIL idle: {} ticks after {} us",
                TICKS.load(Ordering::Relaxed),
                slept / 1000
            );
            return (Ok(false));
        }
        let _: fmt::Result = writeln!(con, "abi-kernel: idle woke after {} us", slept / 1000);

        // An expired timer waits while interrupts are disabled and is
        // delivered when they are enabled again.
        disable();
        set(now());
        let held: u32 = TICKS.load(Ordering::Relaxed);

        enable();
        let delivered: u32 = TICKS.load(Ordering::Relaxed);

        // A cancelled timer never fires.
        set(now() + 1_000_000);
        cancel();
        let spin_until: u64 = now() + 3_000_000;

        while now() < spin_until
        {
            enable();
        }
        let after_cancel: u32 = TICKS.load(Ordering::Relaxed);

        disable();
        if held != 1 || delivered != 2 || after_cancel != 2 || HANDLER_ENABLED.load(Ordering::Relaxed)
        {
            let _: fmt::Result = writeln!(
                con,
                "abi-kernel: FAIL timer delivery: held {} delivered {} after cancel {} nested {}",
                held,
                delivered,
                after_cancel,
                HANDLER_ENABLED.load(Ordering::Relaxed)
            );
            return (Ok(false));
        }
    }
    let _: fmt::Result = writeln!(con, "abi-kernel: timer ok (safe points, cancel, handlers disabled)");
    return (Ok(true));
}

/// Physical page used by the kmap test.
const TEST_PHYS: u64 = 1 << 20;

/// Pattern written through the linear map and read through the window.
const PATTERN: &[u8; 12] = b"liu-physmem!";

/// `kmap`, `kunmap` and `kprotect`: the window aliases physical memory, and
/// bad requests are refused with the documented status.
fn
check_kmem(con: &mut Console, host: &HostOps, boot: &BootInfo) -> Result<bool, Missing>
{
    let kmap: unsafe extern "C" fn(u64, u64, u64, u32) -> Status = need(host.kmap, "kmap")?;
    let kunmap: unsafe extern "C" fn(u64, u64) -> Status = need(host.kunmap, "kunmap")?;
    let kprotect: unsafe extern "C" fn(u64, u64, u32) -> Status = need(host.kprotect, "kprotect")?;
    let (rw, r): (u32, u32) = (PROT_READ | PROT_WRITE, PROT_READ);

    if boot.kmap_size < 4 * PAGE_SIZE || boot.physmem_size < TEST_PHYS + PAGE_SIZE
    {
        let _: fmt::Result = writeln!(con, "abi-kernel: FAIL kmap window {:#x}", boot.kmap_size);
        return (Ok(false));
    }

    let va: u64 = boot.kmap_base;
    let linear: *mut u8 = (boot.physmem_base + TEST_PHYS) as usize as *mut u8;

    // SAFETY: `linear` is inside the linear map of physical memory, and `va`
    // is only accessed after a successful kmap made it readable (and
    // writable where it is written).
    unsafe
    {
        for (i, byte) in PATTERN.iter().enumerate()
        {
            linear.add(i).write_volatile(*byte);
        }

        let mapped: (Status, Status) = (kmap(va, TEST_PHYS, PAGE_SIZE, rw), kmap(va + PAGE_SIZE, TEST_PHYS, PAGE_SIZE, r));

        if mapped != (STATUS_OK, STATUS_OK)
        {
            let _: fmt::Result = writeln!(con, "abi-kernel: FAIL kmap {:?}", mapped);
            return (Ok(false));
        }

        let window: *mut u8 = va as usize as *mut u8;
        let alias: *const u8 = (va + PAGE_SIZE) as usize as *const u8;
        let same: bool = (0..PATTERN.len()).all(|i: usize| -> bool
        {
            return (window.add(i).read_volatile() == PATTERN[i]);
        });

        window.write_volatile(b'L');
        if !same || linear.read_volatile() != b'L' || alias.read_volatile() != b'L'
        {
            let _: fmt::Result = writeln!(con, "abi-kernel: FAIL window does not alias physical memory");
            return (Ok(false));
        }
    }
    let _: fmt::Result = writeln!(con, "abi-kernel: kmap {:#x} aliases phys {:#x}", va, TEST_PHYS);

    // SAFETY: host entries taking plain integers; none of these calls
    // touches memory the kernel is using.
    let results: [(Status, Status); 8] = unsafe
    {
        [
            (kprotect(va, PAGE_SIZE, r), STATUS_OK),
            (kmap(va - PAGE_SIZE, TEST_PHYS, PAGE_SIZE, r), STATUS_FAULT),
            (kmap(va + boot.kmap_size, TEST_PHYS, PAGE_SIZE, r), STATUS_FAULT),
            (kmap(va, boot.physmem_size, PAGE_SIZE, r), STATUS_FAULT),
            (kmap(va, TEST_PHYS, PAGE_SIZE, PROT_WRITE | PROT_EXEC), STATUS_DENIED),
            (kmap(va + 1, TEST_PHYS, PAGE_SIZE, r), STATUS_INVALID),
            (kunmap(va, 2 * PAGE_SIZE), STATUS_OK),
            (kprotect(va, PAGE_SIZE, r), STATUS_FAULT),
        ]
    };

    for (index, (got, want)) in results.iter().enumerate()
    {
        if got != want
        {
            let _: fmt::Result = writeln!(con, "abi-kernel: FAIL kmem case {}: status {} instead of {}", index, got, want);
            return (Ok(false));
        }
    }
    let _: fmt::Result = writeln!(con, "abi-kernel: kmem ok (protect, unmap, refusals)");
    return (Ok(true));
}

/// Returns an address in its own stack frame.
extern "C" fn
stack_probe() -> u64
{
    let marker: u64 = 0;

    return (core::hint::black_box(&marker) as *const u64 as u64);
}

/// Calls `func` with the stack pointer set to `top`, then switches back.
///
/// # Safety
///
/// `top` must be the 16-byte aligned top of a writable stack large enough
/// for `func`.
unsafe fn
call_on_stack(top: u64, func: extern "C" fn() -> u64) -> u64
{
    let result: u64;

    // SAFETY: guaranteed by the caller. r12 is callee-saved, so it keeps the
    // old stack pointer across the call; the target has no red zone.
    unsafe
    {
        core::arch::asm!(
            "mov r12, rsp",
            "mov rsp, {top}",
            "call {func}",
            "mov rsp, r12",
            top = in(reg) top,
            func = in(reg) func,
            out("r12") _,
            lateout("rax") result,
            clobber_abi("C"),
        );
    }

    return (result);
}

/// `kstack_alloc` and `kstack_free`: code runs on an allocated stack.
fn
check_kstack(con: &mut Console, host: &HostOps, _boot: &BootInfo) -> Result<bool, Missing>
{
    let alloc: unsafe extern "C" fn(u64, *mut u64) -> Status = need(host.kstack_alloc, "kstack_alloc")?;
    let free: unsafe extern "C" fn(u64, u64) -> () = need(host.kstack_free, "kstack_free")?;
    let size: u64 = 4 * PAGE_SIZE;
    let mut base: u64 = 0;
    let mut unused: u64 = 0;

    // SAFETY: `base` and `unused` are writable u64s.
    let status: (Status, Status) = unsafe { (alloc(size, &mut base), alloc(0, &mut unused)) };

    if status != (STATUS_OK, STATUS_INVALID) || base == 0 || base % PAGE_SIZE != 0
    {
        let _: fmt::Result = writeln!(con, "abi-kernel: FAIL kstack_alloc {:?} at {:#x}", status, base);
        return (Ok(false));
    }

    // SAFETY: `base + size` is the page-aligned top of a fresh stack of
    // `size` bytes, plenty for `stack_probe`.
    let sp: u64 = unsafe { call_on_stack(base + size, stack_probe) };

    // SAFETY: frees the stack just allocated, which nothing uses any more.
    unsafe
    {
        free(base, size);
    }
    if sp < base || sp >= base + size
    {
        let _: fmt::Result = writeln!(con, "abi-kernel: FAIL code ran at {:#x}, not on the stack {:#x}", sp, base);
        return (Ok(false));
    }
    let _: fmt::Result = writeln!(con, "abi-kernel: kstack ok (ran at {:#x} on a {} KiB stack)", sp, size >> 10);
    return (Ok(true));
}

/// Runs every check and returns the status the entry point reports.
fn
run(host: &HostOps, boot: &BootInfo) -> Status
{
    let mut con: Console = Console { host: host };
    let _: fmt::Result = writeln!(con, "abi-kernel: hello from the LIU ABI test kernel");

    if !check_boot(&mut con, boot)
    {
        return (STATUS_INVALID);
    }

    let checks: [Check; 5] = [check_clocks, check_random, check_timer, check_kmem, check_kstack];

    for check in checks
    {
        match check(&mut con, host, boot)
        {
            Ok(true) =>
            {
            }
            Ok(false) =>
            {
                return (STATUS_INVALID);
            }
            Err(Missing(name)) =>
            {
                let _: fmt::Result = writeln!(con, "abi-kernel: FAIL host does not provide {}", name);
                return (STATUS_UNSUPPORTED);
            }
        }
    }

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

    let mut ops: KernelOps = KernelOps::empty();

    ops.timer = Some(on_timer);

    // SAFETY: checked non-null. The table is written once through the raw
    // pointer and no reference to it is kept, since the runtime reads it
    // while the kernel runs.
    unsafe
    {
        kernel.write(ops);
    }

    // SAFETY: checked non-null; the caller guarantees validity.
    let status: Status = unsafe { run(host_ref, &*boot) };

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
