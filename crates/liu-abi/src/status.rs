// SPDX-License-Identifier: MPL-2.0

//! Status codes returned by host ABI operations.
//!
//! Host `errno` values never cross the boundary. The runtime maps host errors
//! to these codes, and the kernel maps them to Linux errno values.

use core::fmt;

/// Raw status code as it crosses the boundary.
pub type Status = i32;

/// The operation succeeded.
pub const STATUS_OK: Status = 0;

/// An argument was invalid (bad range, bad flags, misaligned address).
pub const STATUS_INVALID: Status = 1;

/// The host could not allocate memory or another resource.
pub const STATUS_NO_MEMORY: Status = 2;

/// A handle did not refer to a live object of the expected kind.
pub const STATUS_BAD_HANDLE: Status = 3;

/// The operation is not provided by this runtime.
pub const STATUS_UNSUPPORTED: Status = 4;

/// The object is busy; retry later.
pub const STATUS_BUSY: Status = 5;

/// A pointer passed to the host was not accessible.
pub const STATUS_FAULT: Status = 6;

/// A host I/O error occurred.
pub const STATUS_IO: Status = 7;

/// The operation would block.
pub const STATUS_AGAIN: Status = 8;

/// The operation was denied by policy (Capsicum, broker).
pub const STATUS_DENIED: Status = 9;

/// The peer speaks an incompatible ABI version.
pub const STATUS_VERSION: Status = 10;

/// The named object does not exist.
pub const STATUS_NOT_FOUND: Status = 11;

/// Rust view of a non-success status.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Error
{
    Invalid,
    NoMemory,
    BadHandle,
    Unsupported,
    Busy,
    Fault,
    Io,
    Again,
    Denied,
    Version,
    NotFound,
    /// A code this version of the ABI does not know.
    Unknown(Status),
}

impl Error
{
    /// Converts a raw status into a result.
    pub const fn
    check(status: Status) -> Result<(), Error>
    {
        let error: Error = match status
        {
            STATUS_OK =>
            {
                return (Ok(()));
            }
            STATUS_INVALID => Error::Invalid,
            STATUS_NO_MEMORY => Error::NoMemory,
            STATUS_BAD_HANDLE => Error::BadHandle,
            STATUS_UNSUPPORTED => Error::Unsupported,
            STATUS_BUSY => Error::Busy,
            STATUS_FAULT => Error::Fault,
            STATUS_IO => Error::Io,
            STATUS_AGAIN => Error::Again,
            STATUS_DENIED => Error::Denied,
            STATUS_VERSION => Error::Version,
            STATUS_NOT_FOUND => Error::NotFound,
            other => Error::Unknown(other),
        };

        return (Err(error));
    }

    /// Returns the raw code for this error.
    pub const fn
    status(self) -> Status
    {
        let status: Status = match self
        {
            Error::Invalid => STATUS_INVALID,
            Error::NoMemory => STATUS_NO_MEMORY,
            Error::BadHandle => STATUS_BAD_HANDLE,
            Error::Unsupported => STATUS_UNSUPPORTED,
            Error::Busy => STATUS_BUSY,
            Error::Fault => STATUS_FAULT,
            Error::Io => STATUS_IO,
            Error::Again => STATUS_AGAIN,
            Error::Denied => STATUS_DENIED,
            Error::Version => STATUS_VERSION,
            Error::NotFound => STATUS_NOT_FOUND,
            Error::Unknown(code) => code,
        };

        return (status);
    }

    /// Converts a Rust result into a raw status.
    pub const fn
    from_result(result: Result<(), Error>) -> Status
    {
        let status: Status = match result
        {
            Ok(()) => STATUS_OK,
            Err(error) => error.status(),
        };

        return (status);
    }
}

impl fmt::Display for Error
{
    fn
    fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
        let text: &str = match self
        {
            Error::Invalid => "invalid argument",
            Error::NoMemory => "out of memory",
            Error::BadHandle => "bad handle",
            Error::Unsupported => "operation not supported",
            Error::Busy => "resource busy",
            Error::Fault => "bad address",
            Error::Io => "I/O error",
            Error::Again => "operation would block",
            Error::Denied => "permission denied",
            Error::Version => "incompatible ABI version",
            Error::NotFound => "not found",
            Error::Unknown(code) =>
            {
                return (write!(f, "unknown status {}", code));
            }
        };

        return (f.write_str(text));
    }
}

impl core::error::Error for Error
{
}

#[cfg(test)]
mod tests
{
    use super::*;

    #[test]
    fn
    round_trips_every_known_code() -> ()
    {
        let mut code: Status = STATUS_INVALID;

        while code <= STATUS_NOT_FOUND
        {
            let error: Error = Error::check(code).unwrap_err();

            assert!(!matches!(error, Error::Unknown(_)), "code {} is unknown", code);
            assert_eq!(error.status(), code);
            code += 1;
        }
    }

    #[test]
    fn
    ok_is_success() -> ()
    {
        assert_eq!(Error::check(STATUS_OK), Ok(()));
        assert_eq!(Error::from_result(Ok(())), STATUS_OK);
    }

    #[test]
    fn
    unknown_codes_are_preserved() -> ()
    {
        let error: Error = Error::check(-7).unwrap_err();

        assert_eq!(error, Error::Unknown(-7));
        assert_eq!(error.status(), -7);
    }
}
