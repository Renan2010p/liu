// SPDX-License-Identifier: MPL-2.0

//! The Renux trap backend on the running kernel.
//!
//! Each test places a few hand-assembled x86_64 instructions of Linux user
//! code in guest physical memory, maps it into a guest space the way the
//! Linux kernel will, runs it, and plays the kernel: it answers the system
//! calls and checks what the guest saw. The tests are skipped on hosts
//! without `PT_SET_SC_RET`.

#![cfg(target_os = "freebsd")]

use liu_abi::trap::{ACCESS_EXEC, ACCESS_READ, ACCESS_WRITE, TRAP_PAGE_FAULT, TRAP_SYSCALL};
use liu_abi::{GuestRegs, TrapEvent};
use liu_platform::{Capabilities, GuestSpace, Mapping, Protection, SharedMemory, TrapBackend};

const PAGE: usize = 4096;

/// Guest layout: code, data and stack pages, backed by physical pages 0-2.
const CODE_VA: u64 = 0x40_0000;
const DATA_VA: u64 = CODE_VA + PAGE as u64;
const STACK_VA: u64 = DATA_VA + PAGE as u64;

/// Linux x86_64 system call numbers used by the guest programs.
const NR_WRITE: u64 = 1;
const NR_EXIT: u64 = 60;

/// Interrupts enabled, reserved bit 1 set: a normal user `rflags`.
const RFLAGS: u64 = 0x202;

/// Guest physical memory holding one program.
struct Guest
{
    /// Read-write view of physical memory, used as the "kernel" would.
    view: Mapping,
    physmem: SharedMemory,
}

impl Guest
{
    fn
    new(program: &[u8]) -> Guest
    {
        let physmem: SharedMemory = SharedMemory::create(3 * PAGE as u64).unwrap();
        let mut view: Mapping = physmem.map(0, 3 * PAGE, Protection::READ_WRITE).unwrap();

        view.write_at(0, program).unwrap();
        return (Guest { view: view, physmem: physmem });
    }

    /// Creates a space and maps code (read-execute), data and stack.
    fn
    space(&self, backend: &mut dyn TrapBackend) -> Box<dyn GuestSpace>
    {
        let mut space: Box<dyn GuestSpace> = backend.create_space(&self.physmem).unwrap();
        let rx: Protection = Protection::READ.union(Protection::EXEC);
        let len: u64 = PAGE as u64;

        space.map(CODE_VA, len, 0, rx).unwrap();
        space.map(DATA_VA, len, len, Protection::READ_WRITE).unwrap();
        space.map(STACK_VA, len, 2 * len, Protection::READ_WRITE).unwrap();
        return (space);
    }

    fn
    code_addr(&self) -> u64
    {
        return (CODE_VA);
    }

    fn
    data_addr(&self) -> u64
    {
        return (DATA_VA);
    }

    fn
    initial_regs(&self) -> GuestRegs
    {
        return (GuestRegs { rip: CODE_VA, rsp: STACK_VA + PAGE as u64, rflags: RFLAGS, ..GuestRegs::default() });
    }
}

/// Returns the Renux backend, or `None` (test skipped) on other kernels.
fn
backend() -> Option<Box<dyn TrapBackend>>
{
    let caps: Capabilities = liu_platform::probe().unwrap();

    if !caps.renux.set_sc_ret
    {
        eprintln!("skip: {} has no PT_SET_SC_RET", caps.os);
        return (None);
    }

    return (Some(liu_platform::open_trap_backend(&caps).unwrap()));
}

/// write(1, msg, 6); exit(write's result), with rsi = rdx and rbx = rcx
/// after the write.
///
/// ```text
///  0: lea  rsi, [rip + 35]     ; msg at 42
///  7: mov  edi, 1
/// 12: mov  edx, 6
/// 17: mov  eax, 1              ; write
/// 22: syscall
/// 24: mov  rdi, rax
/// 27: mov  rsi, rdx            ; rdx must survive the call
/// 30: mov  rbx, rcx            ; rcx must hold the return address
/// 33: mov  eax, 60             ; exit
/// 38: syscall
/// 40: ud2
/// 42: "hello\n"
/// ```
const WRITE_EXIT: [u8; 48] = [
    0x48, 0x8d, 0x35, 0x23, 0x00, 0x00, 0x00, 0xbf, 0x01, 0x00, 0x00, 0x00, 0xba, 0x06, 0x00,
    0x00, 0x00, 0xb8, 0x01, 0x00, 0x00, 0x00, 0x0f, 0x05, 0x48, 0x89, 0xc7, 0x48, 0x89, 0xd6,
    0x48, 0x89, 0xcb, 0xb8, 0x3c, 0x00, 0x00, 0x00, 0x0f, 0x05, 0x0f, 0x0b, b'h', b'e', b'l',
    b'l', b'o', b'\n',
];

#[test]
fn
system_calls_are_trapped_and_answered() -> ()
{
    let Some(mut backend): Option<Box<dyn TrapBackend>> = backend()
    else
    {
        return;
    };
    let guest: Guest = Guest::new(&WRITE_EXIT);
    let code: u64 = guest.code_addr();
    let mut space: Box<dyn GuestSpace> = guest.space(backend.as_mut());
    let mut regs: GuestRegs = guest.initial_regs();
    let mut msg: [u8; 6] = [0; 6];

    // write(1, "hello\n", 6): the host never executes it.
    let event: TrapEvent = space.run(&mut regs).unwrap();

    assert_eq!((event.kind, event.code), (TRAP_SYSCALL, NR_WRITE));
    assert_eq!((regs.rdi, regs.rdx, regs.rip), (1, 6, code + 24));
    assert_eq!(regs.rsi, code + 42);
    assert_eq!((regs.rcx, regs.r11), (code + 24, regs.rflags), "rcx/r11 as left by syscall");
    guest.view.read_at((regs.rsi - code) as usize, &mut msg).unwrap();
    assert_eq!(&msg, b"hello\n");

    // Answer 6; the guest passes it to exit and copies rdx into rsi.
    regs.rax = 6;

    let event: TrapEvent = space.run(&mut regs).unwrap();

    assert_eq!((event.kind, event.code), (TRAP_SYSCALL, NR_EXIT));
    assert_eq!(regs.rdi, 6, "the guest did not receive the injected result");
    assert_eq!(regs.rsi, 6, "rdx was not preserved across the system call");
    assert_eq!(regs.rbx, code + 24, "the guest did not get rcx = return address");
    assert_eq!(regs.rip, code + 40);
}

/// exit(-ENOSYS from system call 999).
///
/// ```text
///  0: mov eax, 999
///  5: syscall
///  7: mov rdi, rax
/// 10: mov eax, 60
/// 15: syscall
/// ```
const UNKNOWN_CALL: [u8; 17] = [
    0xb8, 0xe7, 0x03, 0x00, 0x00, 0x0f, 0x05, 0x48, 0x89, 0xc7, 0xb8, 0x3c, 0x00, 0x00, 0x00, 0x0f,
    0x05,
];

#[test]
fn
errors_are_returned_in_linux_form() -> ()
{
    let Some(mut backend): Option<Box<dyn TrapBackend>> = backend()
    else
    {
        return;
    };
    let guest: Guest = Guest::new(&UNKNOWN_CALL);
    let mut space: Box<dyn GuestSpace> = guest.space(backend.as_mut());
    let mut regs: GuestRegs = guest.initial_regs();

    assert_eq!(space.run(&mut regs).unwrap().code, 999);
    regs.rax = (-38i64) as u64;
    assert_eq!(space.run(&mut regs).unwrap().code, NR_EXIT);
    assert_eq!(regs.rdi as i64, -38);
    assert_eq!(regs.rflags & 1, 0, "the carry flag leaked FreeBSD's error convention");
}

/// Three faults, then exit(fs:[0]).
///
/// ```text
///  0: mov byte [rip + 57], 1     ; write to the read-only code page (64)
///  7: mov byte [0x1000], 1       ; touch of an unmapped page
/// 15: mov rdi, fs:[0]            ; TLS read through fs_base
/// 24: mov eax, 60
/// 29: syscall
/// ```
const FAULTS_THEN_TLS: [u8; 31] = [
    0xc6, 0x05, 0x39, 0x00, 0x00, 0x00, 0x01, 0xc6, 0x04, 0x25, 0x00, 0x10, 0x00, 0x00, 0x01, 0x64,
    0x48, 0x8b, 0x3c, 0x25, 0x00, 0x00, 0x00, 0x00, 0xb8, 0x3c, 0x00, 0x00, 0x00, 0x0f, 0x05,
];

/// Runs until the next trap, which must be a page fault at `addr`.
fn
expect_fault(space: &mut Box<dyn GuestSpace>, regs: &mut GuestRegs, addr: u64, access: u32) -> ()
{
    let event: TrapEvent = space.run(regs).unwrap();

    assert_eq!((event.kind, event.addr, event.access), (TRAP_PAGE_FAULT, addr, access));
}

#[test]
fn
faults_are_reported_and_fs_base_is_honoured() -> ()
{
    let Some(mut backend): Option<Box<dyn TrapBackend>> = backend()
    else
    {
        return;
    };
    let mut guest: Guest = Guest::new(&FAULTS_THEN_TLS);
    let code: u64 = guest.code_addr();
    let mut space: Box<dyn GuestSpace> = guest.space(backend.as_mut());
    let mut regs: GuestRegs = guest.initial_regs();

    guest.view.write_at(PAGE, &0x1122_3344_5566_7788u64.to_le_bytes()).unwrap();

    // Write to a mapped, read-only page.
    expect_fault(&mut space, &mut regs, code + 64, ACCESS_WRITE);
    assert_eq!(regs.rip, code);

    // The "kernel" skips the instruction; the next one touches nothing.
    regs.rip = code + 7;
    expect_fault(&mut space, &mut regs, 0x1000, ACCESS_READ);
    assert_eq!(regs.rip, code + 7);

    // Skip again and install a TLS base.
    regs.rip = code + 15;
    regs.fs_base = guest.data_addr();

    let event: TrapEvent = space.run(&mut regs).unwrap();

    assert_eq!((event.kind, event.code), (TRAP_SYSCALL, NR_EXIT));
    assert_eq!(regs.rdi, 0x1122_3344_5566_7788);
    assert_eq!(regs.fs_base, guest.data_addr());

    // Jump into the data page, which is not executable.
    regs.rip = guest.data_addr();
    expect_fault(&mut space, &mut regs, guest.data_addr(), ACCESS_EXEC);
}

/// getpid() 10000 times in a loop, then exit.
///
/// ```text
///  0: mov ebx, 10000
///  5: mov eax, 39               ; getpid
/// 10: syscall
/// 12: dec ebx
/// 14: jnz 5
/// 16: mov eax, 60
/// 21: syscall
/// ```
const GETPID_LOOP: [u8; 23] = [
    0xbb, 0x10, 0x27, 0x00, 0x00, 0xb8, 0x27, 0x00, 0x00, 0x00, 0x0f, 0x05, 0xff, 0xcb, 0x75, 0xf5,
    0xb8, 0x3c, 0x00, 0x00, 0x00, 0x0f, 0x05,
];

#[test]
fn
many_system_calls_keep_guest_state() -> ()
{
    let Some(mut backend): Option<Box<dyn TrapBackend>> = backend()
    else
    {
        return;
    };
    let guest: Guest = Guest::new(&GETPID_LOOP);
    let mut space: Box<dyn GuestSpace> = guest.space(backend.as_mut());
    let mut regs: GuestRegs = guest.initial_regs();
    let mut calls: u64 = 0;
    let start: std::time::Instant = std::time::Instant::now();

    loop
    {
        let event: TrapEvent = space.run(&mut regs).unwrap();

        assert_eq!(event.kind, TRAP_SYSCALL);
        if event.code == NR_EXIT
        {
            break;
        }
        assert_eq!(event.code, 39);
        calls += 1;
        regs.rax = 1;
    }

    let elapsed: std::time::Duration = start.elapsed();

    assert_eq!(calls, 10_000, "the loop counter in rbx was disturbed");
    eprintln!("renux-setscret: {} system calls in {:?} ({:.1} us each)", calls, elapsed, elapsed.as_secs_f64() * 1e6 / calls as f64);
}

/// Address of this static in the runtime; a guest must not see it.
static RUNTIME_SECRET: u64 = 0x5ec2e7;

#[test]
fn
runtime_memory_is_invisible_to_the_guest() -> ()
{
    let Some(mut backend): Option<Box<dyn TrapBackend>> = backend()
    else
    {
        return;
    };
    let secret: u64 = &RUNTIME_SECRET as *const u64 as usize as u64;
    // mov rax, [secret] (movabs); mov eax, 60; syscall
    let mut program: Vec<u8> = vec![0x48, 0xa1];

    program.extend_from_slice(&secret.to_le_bytes());
    program.extend_from_slice(&[0xb8, 0x3c, 0x00, 0x00, 0x00, 0x0f, 0x05]);

    let guest: Guest = Guest::new(&program);
    let mut space: Box<dyn GuestSpace> = guest.space(backend.as_mut());
    let mut regs: GuestRegs = guest.initial_regs();

    expect_fault(&mut space, &mut regs, secret, ACCESS_READ);
    assert_eq!(regs.rip, CODE_VA);
}

/// mov byte [DATA_VA], 1; mov eax, 60; syscall
const WRITE_DATA: [u8; 15] = [
    0xc6, 0x04, 0x25, 0x00, 0x10, 0x40, 0x00, 0x01, 0xb8, 0x3c, 0x00, 0x00, 0x00, 0x0f, 0x05,
];

#[test]
fn
protect_and_unmap_change_the_guest_view() -> ()
{
    let Some(mut backend): Option<Box<dyn TrapBackend>> = backend()
    else
    {
        return;
    };
    let guest: Guest = Guest::new(&WRITE_DATA);
    let mut space: Box<dyn GuestSpace> = guest.space(backend.as_mut());
    let mut regs: GuestRegs = guest.initial_regs();
    let mut byte: [u8; 1] = [0];

    // Read-only data page: the store faults as a write.
    space.protect(DATA_VA, PAGE as u64, Protection::READ).unwrap();
    expect_fault(&mut space, &mut regs, DATA_VA, ACCESS_WRITE);

    // The "kernel" makes it writable and retries the instruction.
    space.protect(DATA_VA, PAGE as u64, Protection::READ_WRITE).unwrap();
    assert_eq!(space.run(&mut regs).unwrap().code, NR_EXIT);
    guest.view.read_at(PAGE, &mut byte).unwrap();
    assert_eq!(byte, [1], "the store did not reach physical memory");

    // Without the page, the same store finds nothing mapped.
    space.unmap(DATA_VA, PAGE as u64).unwrap();
    regs.rip = CODE_VA;
    expect_fault(&mut space, &mut regs, DATA_VA, ACCESS_READ);

    // Refused before reaching the stub.
    assert!(space.map(0x40_0001, PAGE as u64, 0, Protection::READ).is_err());
    assert!(space.map(DATA_VA, PAGE as u64, 3 * PAGE as u64, Protection::READ).is_err());
    assert!(space.unmap(liu_platform::stub::STUB_CODE, PAGE as u64).is_err());
}
