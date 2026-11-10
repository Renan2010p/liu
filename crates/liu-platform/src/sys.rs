// SPDX-License-Identifier: MPL-2.0

//! Small POSIX helpers shared by every host module.

use std::ffi::CStr;
use std::io;
use std::mem::MaybeUninit;

use crate::caps::HostOs;

/// Converts a `-1` return value into the current `errno`.
pub(crate) fn
check(ret: libc::c_int) -> io::Result<libc::c_int>
{
    if ret == -1
    {
        return (Err(io::Error::last_os_error()));
    }

    return (Ok(ret));
}

/// `uname(3)` system name, release and machine.
pub(crate) struct Uname
{
    pub os: HostOs,
    pub release: String,
    pub machine: String,
}

fn
field(raw: &[libc::c_char]) -> String
{
    // SAFETY: uname(3) NUL-terminates every field within its array.
    let text: &CStr = unsafe { CStr::from_ptr(raw.as_ptr()) };

    return (text.to_string_lossy().into_owned());
}

pub(crate) fn
uname() -> io::Result<Uname>
{
    let mut raw: MaybeUninit<libc::utsname> = MaybeUninit::uninit();

    // SAFETY: `raw` is a valid, writable utsname buffer.
    check(unsafe { libc::uname(raw.as_mut_ptr()) })?;

    // SAFETY: uname(3) succeeded and initialized the structure.
    let raw: libc::utsname = unsafe { raw.assume_init() };
    let info: Uname = Uname
    {
        os: HostOs::from_sysname(&field(&raw.sysname)),
        release: field(&raw.release),
        machine: field(&raw.machine),
    };

    return (Ok(info));
}
