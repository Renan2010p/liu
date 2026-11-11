// SPDX-License-Identifier: MPL-2.0

//! Fixed-address memory on the running host.

use std::io;

use liu_platform::{Mapping, Protection, Region, SharedMemory};

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
