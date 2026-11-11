// SPDX-License-Identifier: MPL-2.0

//! Host clocks and entropy.

use std::io;
use std::mem::MaybeUninit;

use crate::host;

fn
clock_ns(clock: libc::clockid_t) -> u64
{
    let mut ts: MaybeUninit<libc::timespec> = MaybeUninit::uninit();

    // SAFETY: `ts` is a valid output buffer.
    let ret: libc::c_int = unsafe { libc::clock_gettime(clock, ts.as_mut_ptr()) };

    if ret != 0
    {
        return (0);
    }

    // SAFETY: clock_gettime succeeded and filled `ts`.
    let ts: libc::timespec = unsafe { ts.assume_init() };
    let ns: u64 = (ts.tv_sec as u64).wrapping_mul(1_000_000_000).wrapping_add(ts.tv_nsec as u64);

    return (ns);
}

/// Nanoseconds of the host monotonic clock (arbitrary origin, never goes
/// backwards).
pub fn
clock_monotonic_ns() -> u64
{
    return (clock_ns(libc::CLOCK_MONOTONIC));
}

/// Nanoseconds since the Unix epoch.
pub fn
clock_realtime_ns() -> u64
{
    return (clock_ns(libc::CLOCK_REALTIME));
}

/// Fills `buf` with cryptographically secure random bytes from the host.
pub fn
fill_random(buf: &mut [u8]) -> io::Result<()>
{
    let mut done: usize = 0;

    while done < buf.len()
    {
        match host::random_chunk(&mut buf[done..])
        {
            Ok(n) =>
            {
                done += n;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted =>
            {
                continue;
            }
            Err(error) =>
            {
                return (Err(error));
            }
        }
    }

    return (Ok(()));
}
