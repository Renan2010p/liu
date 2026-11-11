// SPDX-License-Identifier: MPL-2.0

//! Host clocks and entropy.

use liu_platform::{clock_monotonic_ns, clock_realtime_ns, fill_random};

/// 2020-01-01T00:00:00Z in nanoseconds.
const YEAR_2020_NS: u64 = 1_577_836_800 * 1_000_000_000;

#[test]
fn
monotonic_clock_advances() -> ()
{
    let first: u64 = clock_monotonic_ns();

    std::thread::sleep(std::time::Duration::from_millis(2));
    assert!(clock_monotonic_ns() >= first + 1_000_000);
}

#[test]
fn
realtime_clock_is_plausible() -> ()
{
    assert!(clock_realtime_ns() > YEAR_2020_NS);
}

#[test]
fn
fills_large_random_buffers() -> ()
{
    let mut a: Vec<u8> = vec![0; 4096];
    let mut b: Vec<u8> = vec![0; 4096];

    fill_random(&mut a).unwrap();
    fill_random(&mut b).unwrap();
    assert_ne!(a, b);
    assert!(a.iter().any(|x: &u8| -> bool { return (*x != 0); }));
}
