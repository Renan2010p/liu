// SPDX-License-Identifier: MPL-2.0

//! Shared memory on the running host.

use std::io;

use liu_platform::{Mapping, Protection, SharedMemory};

const SIZE: u64 = 1 << 20;

#[test]
fn
creates_object_of_requested_size() -> ()
{
    let shm: SharedMemory = SharedMemory::create(SIZE).unwrap();

    assert_eq!(shm.size(), SIZE);
}

#[test]
fn
rejects_empty_object() -> ()
{
    let error: io::Error = SharedMemory::create(0).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn
two_mappings_see_the_same_bytes() -> ()
{
    let shm: SharedMemory = SharedMemory::create(SIZE).unwrap();
    let mut writer: Mapping = shm.map(0, SIZE as usize, Protection::READ_WRITE).unwrap();
    let reader: Mapping = shm.map(4096, 4096, Protection::READ).unwrap();
    let mut buf: [u8; 5] = [0; 5];

    writer.write_at(4096 + 10, b"hello").unwrap();
    reader.read_at(10, &mut buf).unwrap();
    assert_eq!(&buf, b"hello");
}

#[test]
fn
new_object_is_zero_filled() -> ()
{
    let shm: SharedMemory = SharedMemory::create(SIZE).unwrap();
    let map: Mapping = shm.map(0, 4096, Protection::READ).unwrap();
    let mut buf: [u8; 4096] = [0xff; 4096];

    map.read_at(0, &mut buf).unwrap();
    assert!(buf.iter().all(|b: &u8| -> bool { return (*b == 0); }));
}

#[test]
fn
rejects_out_of_range_access() -> ()
{
    let shm: SharedMemory = SharedMemory::create(SIZE).unwrap();
    let mut map: Mapping = shm.map(0, 4096, Protection::READ_WRITE).unwrap();
    let mut buf: [u8; 8] = [0; 8];

    assert!(shm.map(SIZE - 4096, 8192, Protection::READ).is_err());
    assert!(map.read_at(4090, &mut buf).is_err());
    assert!(map.write_at(usize::MAX, &buf).is_err());
}
