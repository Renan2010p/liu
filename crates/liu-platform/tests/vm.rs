// SPDX-License-Identifier: MPL-2.0

//! Fixed-address memory on the running host.

use std::io;

use liu_platform::{page_size, Child, ForkResult, Mapping, Protection, Region};
use liu_platform::{SharedMemory, Stack, WaitStatus, Window};

const LEN: usize = 1 << 16;

/// Attempts before a test gives up looking for a free address.
const ATTEMPTS: usize = 100;

/// Finds a page-aligned address that was free a moment ago, by mapping and
/// dropping memory at a host-chosen address.
///
/// Tests run in parallel, so another thread may take the address before
/// the caller maps it: callers retry on `AddrInUse` with [`at_free_address`].
fn
free_address(len: usize) -> u64
{
    let shm: SharedMemory = SharedMemory::create(len as u64).unwrap();
    let probe: Mapping = shm.map(0, len, Protection::READ).unwrap();

    return (probe.as_ptr() as usize as u64);
}

/// Calls `map` with free addresses until it does not fail with `AddrInUse`.
fn
at_free_address<T>(map: &dyn Fn(u64) -> io::Result<T>) -> (u64, T)
{
    for _ in 0..ATTEMPTS
    {
        let addr: u64 = free_address(LEN);

        match map(addr)
        {
            Ok(value) =>
            {
                let found: (u64, T) = (addr, value);

                return (found);
            }
            Err(error) if error.kind() == io::ErrorKind::AddrInUse =>
            {
                continue;
            }
            Err(error) =>
            {
                panic!("mapping at {:#x} failed: {}", addr, error);
            }
        }
    }
    panic!("no free address after {} attempts", ATTEMPTS);
}

fn
reserve(addr: u64) -> io::Result<Region>
{
    return (Region::reserve_at(addr, LEN));
}

#[test]
fn
reserves_at_requested_address() -> ()
{
    let (addr, region): (u64, Region) = at_free_address(&reserve);

    assert_eq!(region.addr(), addr);
    assert_eq!(region.len(), LEN);
}

#[test]
fn
refuses_occupied_address() -> ()
{
    let (addr, _first): (u64, Region) = at_free_address(&reserve);
    let error: io::Error = Region::reserve_at(addr, LEN).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
}

#[test]
fn
refuses_unaligned_address() -> ()
{
    let error: io::Error = Region::reserve_at(0x1001, LEN).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn
protect_seals_the_region() -> ()
{
    let (_, mut region): (u64, Region) = at_free_address(&reserve);

    region.write_at(0, b"code").unwrap();
    region.write_at(LEN - 4, b"tail").unwrap();
    assert_eq!(region.write_at(LEN - 3, b"tail").unwrap_err().kind(), io::ErrorKind::InvalidInput);
    region.protect(0, 4096, Protection::READ).unwrap();
    assert_eq!(region.write_at(8192, b"x").unwrap_err().kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn
shared_memory_maps_at_requested_address() -> ()
{
    let shm: SharedMemory = SharedMemory::create(LEN as u64).unwrap();
    let (addr, mut fixed): (u64, Mapping) = at_free_address(&|addr: u64| -> io::Result<Mapping>
    {
        return (shm.map_at(0, LEN, Protection::READ_WRITE, addr));
    });
    let other: Mapping = shm.map(0, LEN, Protection::READ).unwrap();
    let mut buf: [u8; 2] = [0; 2];

    assert_eq!(fixed.as_ptr() as usize as u64, addr);
    fixed.write_at(100, b"ok").unwrap();
    other.read_at(100, &mut buf).unwrap();
    assert_eq!(&buf, b"ok");
}

fn
reserve_window(addr: u64) -> io::Result<Window>
{
    return (Window::reserve_at(addr, LEN));
}

#[test]
fn
window_maps_and_unmaps_shared_pages() -> ()
{
    let page: usize = page_size();
    let shm: SharedMemory = SharedMemory::create(LEN as u64).unwrap();
    let mut view: Mapping = shm.map(0, LEN, Protection::READ_WRITE).unwrap();
    let (addr, mut window): (u64, Window) = at_free_address(&reserve_window);
    let mut buf: [u8; 5] = [0; 5];

    assert_eq!(window.addr(), addr);
    view.write_at(2 * page, b"liu!!").unwrap();
    window.map_shared(page, &shm, 2 * page as u64, page, Protection::READ_WRITE).unwrap();

    // SAFETY: the page at `addr + page` was just mapped read-write.
    unsafe
    {
        let at: *mut u8 = (addr as usize + page) as *mut u8;

        assert_eq!(std::ptr::read_volatile(at), b'l');
        std::ptr::write_volatile(at.add(4), b'?');
    }
    view.read_at(2 * page, &mut buf).unwrap();
    assert_eq!(&buf, b"liu!?");

    window.protect(page, page, Protection::READ).unwrap();
    window.unmap(page, page).unwrap();
}

#[test]
fn
window_refuses_ranges_outside_it() -> ()
{
    let page: usize = page_size();
    let shm: SharedMemory = SharedMemory::create(LEN as u64).unwrap();
    let (_, mut window): (u64, Window) = at_free_address(&reserve_window);

    assert!(window.map_shared(LEN, &shm, 0, page, Protection::READ).is_err());
    assert!(window.map_shared(0, &shm, LEN as u64, page, Protection::READ).is_err());
    assert!(window.map_shared(1, &shm, 0, page, Protection::READ).is_err());
    assert!(window.unmap(LEN - page, 2 * page).is_err());
    assert!(window.protect(0, 0, Protection::READ).is_err());
}

#[test]
fn
stack_has_a_guard_page_below() -> ()
{
    let page: usize = page_size();
    let stack: Stack = Stack::alloc(4 * page).unwrap();

    assert_eq!(stack.size(), 4 * page);
    assert_eq!(stack.guard() + page as u64, stack.base());
    assert_eq!(stack.base() % page as u64, 0);

    // SAFETY: the usable part of the stack is mapped read-write.
    unsafe
    {
        let top: *mut u64 = (stack.base() as usize + stack.size() - 8) as *mut u64;

        std::ptr::write_volatile(top, 42);
        std::ptr::write_volatile(stack.base() as usize as *mut u64, 7);
        assert_eq!(std::ptr::read_volatile(top), 42);
    }
    assert!(Stack::alloc(0).is_err());
    assert!(Stack::alloc(page + 1).is_err());
}

#[test]
fn
stack_overflow_hits_the_guard_page() -> ()
{
    let stack: Stack = Stack::alloc(page_size()).unwrap();
    let guard: u64 = stack.guard();

    // SAFETY: the child only calls async-signal-safe functions.
    let fork: ForkResult = unsafe { liu_platform::fork().unwrap() };
    let mut child: Child = match fork
    {
        ForkResult::Child =>
        {
            let no_core: libc::rlimit = libc::rlimit { rlim_cur: 0, rlim_max: 0 };

            // SAFETY: setrlimit and a volatile store are async-signal-safe;
            // the store is expected to fault.
            unsafe
            {
                libc::setrlimit(libc::RLIMIT_CORE, &no_core);
                std::ptr::write_volatile(guard as usize as *mut u8, 1);
                libc::_exit(0);
            }
        }
        ForkResult::Parent(child) => child,
    };
    let status: WaitStatus = child.wait().unwrap();

    assert!(
        status == WaitStatus::Signaled(libc::SIGSEGV) || status == WaitStatus::Signaled(libc::SIGBUS),
        "child ended with {:?}",
        status
    );
}
