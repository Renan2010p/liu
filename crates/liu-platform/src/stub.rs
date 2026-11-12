// SPDX-License-Identifier: MPL-2.0

//! The guest stub (architecture.md §5.3).
//!
//! Every guest space contains two pages at a fixed address above the guest
//! address space:
//!
//! ```text
//! STUB_CODE  stub code, read-execute    shared with the runtime
//! STUB_DATA  operation batch, read-write shared with the runtime
//! ```
//!
//! Everything else in the host process is unmapped when the space is
//! created, so a guest sees only the stub and the physical memory the Linux
//! kernel maps for it.
//!
//! The stub executes a batch of host system calls (`mmap`, `munmap`,
//! `mprotect`) written by the runtime into the data page, stores each result
//! in the batch as a value or `-errno`, and stops with `int3`. It never
//! touches the stack, which does not exist after creation. The same code
//! works on every x86_64 host: FreeBSD-style errors (carry flag set, `rax`
//! = errno) are converted to the Linux form.
//!
//! Data page layout, in `u64` words:
//!
//! ```text
//! [0]          number of operations
//! [1]          reserved
//! [2 + 8 * i]  operation i: number, six arguments, result
//! ```

use std::io;
use std::os::fd::{AsFd, AsRawFd};

use crate::host;
use crate::shm::{Mapping, Protection, SharedMemory};

/// Descriptor number of physical memory inside every guest space.
pub const GUEST_PHYSMEM_FD: libc::c_int = 3;

/// Address of the stub code page in every guest space (as in arch/um).
pub const STUB_CODE: u64 = 0x7f_bfff_e000;

/// Address of the stub data page.
pub const STUB_DATA: u64 = STUB_CODE + PAGE;

/// First address after the stub.
pub const STUB_END: u64 = STUB_DATA + PAGE;

/// Guest user addresses must lie below this limit (the stub).
pub const GUEST_TOP: u64 = STUB_CODE;

const PAGE: u64 = 4096;

/// Words per operation: number, six arguments, result.
const OP_WORDS: usize = 8;

/// Operations that fit in one batch.
pub const MAX_OPS: usize = (PAGE as usize / 8 - 2) / OP_WORDS;

// The stub. Position-independent: it is copied to STUB_CODE, and reaches
// the data page one page further, so `liu_stub_data` is defined relative to
// the start of the code.
core::arch::global_asm!(
    ".pushsection .rodata.liu_stub, \"a\"",
    ".balign 16",
    "liu_stub_start:",
    "    lea r12, [rip + liu_stub_start + 4096]",
    "    mov r13, [r12]",
    "    lea r14, [r12 + 16]",
    "2:",
    "    test r13, r13",
    "    jz 4f",
    "    mov rax, [r14]",
    "    mov rdi, [r14 + 8]",
    "    mov rsi, [r14 + 16]",
    "    mov rdx, [r14 + 24]",
    "    mov r10, [r14 + 32]",
    "    mov r8, [r14 + 40]",
    "    mov r9, [r14 + 48]",
    "    syscall",
    "    jnc 3f",
    "    neg rax",
    "3:",
    "    mov [r14 + 56], rax",
    "    add r14, 64",
    "    dec r13",
    "    jmp 2b",
    "4:",
    "    int3",
    "    jmp liu_stub_start",
    "liu_stub_end:",
    ".popsection",
    ".pushsection .data.rel.ro.liu_stub, \"aw\"",
    ".balign 8",
    "liu_stub_bounds:",
    "    .quad liu_stub_start",
    "    .quad liu_stub_end",
    ".popsection",
);

extern "C"
{
    /// Start and end of the stub code, filled by the linker.
    static liu_stub_bounds: [*const u8; 2];
}

/// Returns the stub machine code.
fn
stub_code() -> &'static [u8]
{
    // SAFETY: `liu_stub_bounds` holds the start and end of the stub code in
    // read-only data, which lives for the whole program.
    let code: &'static [u8] = unsafe
    {
        let [start, end]: [*const u8; 2] = liu_stub_bounds;

        std::slice::from_raw_parts(start, end as usize - start as usize)
    };

    return (code);
}

/// One host system call for the stub.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StubOp
{
    pub nr: u64,
    pub args: [u64; 6],
}

/// The two stub pages of one guest space, seen from the runtime.
#[derive(Debug)]
pub struct StubPages
{
    code: SharedMemory,
    data: SharedMemory,
    view: Mapping,
}

impl StubPages
{
    /// Creates the pages; the code page holds the stub.
    pub fn
    new() -> io::Result<StubPages>
    {
        let code: SharedMemory = SharedMemory::create(PAGE)?;
        let data: SharedMemory = SharedMemory::create(PAGE)?;
        let mut code_view: Mapping = code.map(0, PAGE as usize, Protection::READ_WRITE)?;
        let view: Mapping = data.map(0, PAGE as usize, Protection::READ_WRITE)?;

        code_view.write_at(0, stub_code())?;
        return (Ok(StubPages { code: code, data: data, view: view }));
    }

    /// Descriptor of the code page object.
    pub fn
    code_fd(&self) -> libc::c_int
    {
        return (self.code.as_fd().as_raw_fd());
    }

    /// Descriptor of the data page object.
    pub fn
    data_fd(&self) -> libc::c_int
    {
        return (self.data.as_fd().as_raw_fd());
    }

    /// Writes a batch for the stub to run.
    pub fn
    write_batch(&mut self, ops: &[StubOp]) -> io::Result<()>
    {
        if ops.len() > MAX_OPS
        {
            return (Err(io::Error::from(io::ErrorKind::InvalidInput)));
        }

        let mut words: Vec<u64> = vec![ops.len() as u64, 0];

        for op in ops
        {
            words.push(op.nr);
            words.extend_from_slice(&op.args);
            words.push(u64::MAX);
        }

        let bytes: Vec<u8> = words.iter().flat_map(|w: &u64| -> [u8; 8] { return (w.to_le_bytes()); }).collect();

        return (self.view.write_at(0, &bytes));
    }

    /// Reads the results of the last batch of `count` operations: a value,
    /// or `-errno`.
    pub fn
    results(&self, count: usize) -> io::Result<Vec<i64>>
    {
        let mut out: Vec<i64> = Vec::with_capacity(count);

        for index in 0..count
        {
            let mut word: [u8; 8] = [0; 8];

            self.view.read_at((2 + index * OP_WORDS + OP_WORDS - 1) * 8, &mut word)?;
            out.push(i64::from_le_bytes(word));
        }

        return (Ok(out));
    }
}

/// Converts batch results into the first error, if any.
pub fn
check_results(results: &[i64]) -> io::Result<()>
{
    for result in results
    {
        if *result < 0 && *result > -4096
        {
            return (Err(io::Error::from_raw_os_error(-*result as i32)));
        }
    }

    return (Ok(()));
}

/// Address-space changes queued for one guest space, validated before the
/// stub ever sees them.
#[derive(Debug)]
pub struct StubQueue
{
    pages: StubPages,
    ops: Vec<StubOp>,
    physmem_size: u64,
}

fn
invalid() -> io::Error
{
    return (io::Error::from(io::ErrorKind::InvalidInput));
}

/// Checks a page-aligned, non-empty guest range below the stub.
fn
check_guest_range(vaddr: u64, len: u64) -> io::Result<()>
{
    let end: Option<u64> = vaddr.checked_add(len);

    if len == 0 || vaddr % PAGE != 0 || len % PAGE != 0 || vaddr < PAGE || end.is_none()
        || end.unwrap_or(u64::MAX) > GUEST_TOP
    {
        return (Err(invalid()));
    }

    return (Ok(()));
}

impl StubQueue
{
    /// An empty queue for a space backed by `physmem_size` bytes.
    pub fn
    new(physmem_size: u64) -> io::Result<StubQueue>
    {
        return (Ok(StubQueue { pages: StubPages::new()?, ops: Vec::new(), physmem_size: physmem_size }));
    }

    /// The stub pages.
    pub fn
    pages(&mut self) -> &mut StubPages
    {
        return (&mut self.pages);
    }

    /// Number of queued operations.
    pub fn
    len(&self) -> usize
    {
        return (self.ops.len());
    }

    /// Returns whether nothing is queued.
    pub fn
    is_empty(&self) -> bool
    {
        return (self.ops.is_empty());
    }

    /// Returns whether the queue must be flushed before the next push.
    pub fn
    is_full(&self) -> bool
    {
        return (self.ops.len() >= MAX_OPS);
    }

    /// Removes and returns the queued operations.
    pub fn
    take(&mut self) -> Vec<StubOp>
    {
        return (std::mem::take(&mut self.ops));
    }

    /// Queues mapping physical memory `[phys, phys + len)` at `vaddr`.
    pub fn
    map(&mut self, vaddr: u64, len: u64, phys: u64, prot: Protection) -> io::Result<()>
    {
        let end: Option<u64> = phys.checked_add(len);

        check_guest_range(vaddr, len)?;
        if phys % PAGE != 0 || end.is_none() || end.unwrap_or(u64::MAX) > self.physmem_size
        {
            return (Err(invalid()));
        }

        let flags: u64 = (libc::MAP_SHARED | libc::MAP_FIXED) as u64;
        let args: [u64; 6] = [vaddr, len, prot.to_host() as u64, flags, GUEST_PHYSMEM_FD as u64, phys];

        self.ops.push(StubOp { nr: host::SYS_MMAP, args: args });
        return (Ok(()));
    }

    /// Queues removing the mappings of `[vaddr, vaddr + len)`.
    pub fn
    unmap(&mut self, vaddr: u64, len: u64) -> io::Result<()>
    {
        check_guest_range(vaddr, len)?;
        self.ops.push(StubOp { nr: host::SYS_MUNMAP, args: [vaddr, len, 0, 0, 0, 0] });
        return (Ok(()));
    }

    /// Queues changing the protection of `[vaddr, vaddr + len)`.
    pub fn
    protect(&mut self, vaddr: u64, len: u64, prot: Protection) -> io::Result<()>
    {
        check_guest_range(vaddr, len)?;
        self.ops.push(StubOp { nr: host::SYS_MPROTECT, args: [vaddr, len, prot.to_host() as u64, 0, 0, 0] });
        return (Ok(()));
    }

    /// Queues removing everything but the stub from a fresh space.
    pub fn
    cleanup(&mut self) -> ()
    {
        self.ops.push(StubOp { nr: host::SYS_MUNMAP, args: [PAGE, STUB_CODE - PAGE, 0, 0, 0, 0] });
        self.ops.push(StubOp { nr: host::SYS_MUNMAP, args: [STUB_END, host::USER_TOP - STUB_END, 0, 0, 0, 0] });
    }
}

#[cfg(test)]
mod tests
{
    use super::*;

    #[test]
    fn
    queue_validates_ranges() -> ()
    {
        let mut queue: StubQueue = StubQueue::new(1 << 20).unwrap();
        let rw: Protection = Protection::READ_WRITE;

        assert!(queue.map(0x40_0000, 0x1000, 0, rw).is_ok());
        assert!(queue.map(0, 0x1000, 0, rw).is_err());
        assert!(queue.map(0x40_0001, 0x1000, 0, rw).is_err());
        assert!(queue.map(0x40_0000, 0x1000, 1 << 20, rw).is_err());
        assert!(queue.map(STUB_CODE - 0x1000, 0x2000, 0, rw).is_err());
        assert!(queue.unmap(STUB_CODE, 0x1000).is_err());
        assert!(queue.protect(0x40_0000, 0, rw).is_err());
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn
    stub_code_fits_and_ends_with_int3_loop() -> ()
    {
        let code: &[u8] = stub_code();

        assert!(code.len() > 16 && code.len() < 256, "stub is {} bytes", code.len());
        assert!(code.contains(&0xcc), "no int3 in the stub");
        assert_eq!(&code[..3], &[0x4c, 0x8d, 0x25], "stub must start with lea r12, [rip + ...]");
    }

    #[test]
    fn
    stub_reaches_the_data_page() -> ()
    {
        let code: &[u8] = stub_code();
        let disp: i32 = i32::from_le_bytes([code[3], code[4], code[5], code[6]]);

        // lea is 7 bytes long; the target must be code start + one page.
        assert_eq!(7 + disp as i64, PAGE as i64);
    }

    #[test]
    fn
    batches_round_trip() -> ()
    {
        let mut pages: StubPages = StubPages::new().unwrap();
        let op: StubOp = StubOp { nr: 73, args: [1, 2, 3, 4, 5, 6] };

        pages.write_batch(&[op, op]).unwrap();
        assert_eq!(pages.results(2).unwrap(), vec![-1, -1]);
        assert!(pages.write_batch(&vec![op; MAX_OPS + 1]).is_err());
        assert_eq!(MAX_OPS, 63);
    }

    #[test]
    fn
    first_error_wins() -> ()
    {
        assert!(check_results(&[0, 0x1000]).is_ok());
        assert_eq!(check_results(&[0, -12, -22]).unwrap_err().raw_os_error(), Some(12));
    }
}
