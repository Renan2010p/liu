// SPDX-License-Identifier: MPL-2.0

//! Header shared by every table that crosses the boundary.

use crate::status::Error;
use crate::{ABI_MAGIC, ABI_VERSION_MAJOR, ABI_VERSION_MINOR};

/// First member of every boundary table.
///
/// `size` is the size of the whole table as the producer compiled it. A
/// consumer built against a newer minor version treats every entry that lies
/// beyond `size` as absent, which is what makes appending entries a
/// compatible change.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableHeader
{
    pub magic: u32,
    pub abi_major: u16,
    pub abi_minor: u16,
    pub size: u32,
    pub reserved: u32,
}

const _: () = assert!(size_of::<TableHeader>() == 16);

/// Outcome of a successful compatibility check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Compat
{
    /// Highest minor version both sides understand.
    pub minor: u16,
}

impl TableHeader
{
    /// Builds the header for a table of `size` bytes at the current version.
    pub const fn
    new(size: usize) -> TableHeader
    {
        assert!(size <= u32::MAX as usize);

        let header: TableHeader = TableHeader
        {
            magic: ABI_MAGIC,
            abi_major: ABI_VERSION_MAJOR,
            abi_minor: ABI_VERSION_MINOR,
            size: size as u32,
            reserved: 0,
        };

        return (header);
    }

    /// Checks a header received from the other side of the boundary.
    pub const fn
    check(&self) -> Result<Compat, Error>
    {
        if self.magic != ABI_MAGIC
        {
            return (Err(Error::Invalid));
        }
        if self.abi_major != ABI_VERSION_MAJOR
        {
            return (Err(Error::Version));
        }
        if (self.size as usize) < size_of::<TableHeader>()
        {
            return (Err(Error::Invalid));
        }

        let minor: u16 = if self.abi_minor < ABI_VERSION_MINOR
        {
            self.abi_minor
        }
        else
        {
            ABI_VERSION_MINOR
        };

        return (Ok(Compat { minor: minor }));
    }

    /// Returns whether a member ending at byte `end` lies inside the table.
    pub const fn
    covers(&self, end: usize) -> bool
    {
        return (end <= self.size as usize);
    }
}

/// Returns whether `$table` (whose type is `$ty`) provides the entry
/// `$field`: the entry lies within the size the producer declared and is not
/// null.
#[macro_export]
macro_rules! provides
{
    ($table:expr, $ty:ty, $field:ident) =>
    {
        ($table.header.covers(
            ::core::mem::offset_of!($ty, $field) + ::core::mem::size_of_val(&$table.$field),
        ) && $table.$field.is_some())
    };
}

#[cfg(test)]
mod tests
{
    use super::*;

    #[test]
    fn
    accepts_current_version() -> ()
    {
        let header: TableHeader = TableHeader::new(64);

        assert_eq!(header.check(), Ok(Compat { minor: ABI_VERSION_MINOR }));
    }

    #[test]
    fn
    rejects_bad_magic() -> ()
    {
        let mut header: TableHeader = TableHeader::new(64);

        header.magic = 0;
        assert_eq!(header.check(), Err(Error::Invalid));
    }

    #[test]
    fn
    rejects_other_major() -> ()
    {
        let mut header: TableHeader = TableHeader::new(64);

        header.abi_major = ABI_VERSION_MAJOR + 1;
        assert_eq!(header.check(), Err(Error::Version));
    }

    #[test]
    fn
    rejects_truncated_table() -> ()
    {
        let header: TableHeader = TableHeader::new(8);

        assert_eq!(header.check(), Err(Error::Invalid));
    }

    #[test]
    fn
    negotiates_lowest_minor() -> ()
    {
        let mut older: TableHeader = TableHeader::new(64);
        let mut newer: TableHeader = TableHeader::new(64);

        older.abi_minor = 0;
        newer.abi_minor = ABI_VERSION_MINOR + 3;
        assert_eq!(older.check(), Ok(Compat { minor: 0 }));
        assert_eq!(newer.check(), Ok(Compat { minor: ABI_VERSION_MINOR }));
    }

    #[test]
    fn
    covers_respects_declared_size() -> ()
    {
        let header: TableHeader = TableHeader::new(32);

        assert!(header.covers(32));
        assert!(!header.covers(33));
    }
}
