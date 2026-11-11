// SPDX-License-Identifier: MPL-2.0

//! Fixed-address memory on the running host.

use std::io;

use liu_platform::{Mapping, Protection, Region, SharedMemory};

const LEN: usize = 1 << 16;

/// Finds a free, page-aligned address by mapping and dropping a region at
/// a host-chosen address.
fn
free_address(len: usize) -> u64
{
    let shm: SharedMemory = SharedMemory::create(len as u64).unwrap();
    let probe: Mapping = shm.map(0, len, Protection::READ).unwrap();

    return (probe.as_ptr() as usize as u64);
}

#[test]
fn
reserves_at_requested_address() -> ()
{
    let addr: u64 = free_address(LEN);
    let region: Region = Region::reserve_at(addr, LEN).unwrap();

    assert_eq!(region.addr(), addr);
    assert_eq!(region.len(), LEN);
}

#[test]
fn
refuses_occupied_address() -> ()
{
    let addr: u64 = free_address(LEN);
    let _first: Region = Region::reserve_at(addr, LEN).unwrap();
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
    let addr: u64 = free_address(LEN);
    let mut region: Region = Region::reserve_at(addr, LEN).unwrap();

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
    let addr: u64 = free_address(LEN);
    let mut fixed: Mapping = shm.map_at(0, LEN, Protection::READ_WRITE, addr).unwrap();
    let other: Mapping = shm.map(0, LEN, Protection::READ).unwrap();
    let mut buf: [u8; 2] = [0; 2];

    assert_eq!(fixed.as_ptr() as usize as u64, addr);
    fixed.write_at(100, b"ok").unwrap();
    other.read_at(100, &mut buf).unwrap();
    assert_eq!(&buf, b"ok");
}
