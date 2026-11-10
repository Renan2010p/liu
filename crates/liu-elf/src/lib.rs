// SPDX-License-Identifier: MPL-2.0

//! ELF64 parsing and load planning for the LIU kernel image.
//!
//! The parser never trusts the input: every offset and size is checked
//! against the buffer, and arithmetic is overflow-checked. [`plan_kernel`]
//! applies the stricter rules a LIU kernel image must satisfy.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

#[cfg(test)]
extern crate std;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

pub const ET_EXEC: u16 = 2;
pub const ET_DYN: u16 = 3;

pub const EM_X86_64: u16 = 62;
pub const EM_AARCH64: u16 = 183;

pub const PT_LOAD: u32 = 1;
pub const PT_DYNAMIC: u32 = 2;
pub const PT_INTERP: u32 = 3;

pub const PF_X: u32 = 0x1;
pub const PF_W: u32 = 0x2;
pub const PF_R: u32 = 0x4;

const EHDR_SIZE: usize = 64;
const PHDR_SIZE: usize = 56;
const PAGE_SIZE: u64 = 4096;

/// One `PT_LOAD` segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment
{
    pub flags: u32,
    pub offset: u64,
    pub vaddr: u64,
    pub filesz: u64,
    pub memsz: u64,
    pub align: u64,
}

impl Segment
{
    /// Returns whether the segment is executable.
    pub fn
    is_exec(&self) -> bool
    {
        return (self.flags & PF_X != 0);
    }

    /// Returns whether the segment is writable.
    pub fn
    is_write(&self) -> bool
    {
        return (self.flags & PF_W != 0);
    }
}

/// Parsed ELF64 little-endian file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Elf
{
    pub kind: u16,
    pub machine: u16,
    pub entry: u64,
    pub interpreter: Option<String>,
    pub has_dynamic: bool,
    pub segments: Vec<Segment>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError
{
    Truncated,
    BadMagic,
    UnsupportedClass,
    UnsupportedEndianness,
    BadProgramHeader,
}

impl fmt::Display for ParseError
{
    fn
    fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
        let text: &str = match self
        {
            ParseError::Truncated => "truncated ELF file",
            ParseError::BadMagic => "not an ELF file",
            ParseError::UnsupportedClass => "not a 64-bit ELF file",
            ParseError::UnsupportedEndianness => "not a little-endian ELF file",
            ParseError::BadProgramHeader => "malformed program header table",
        };

        return (f.write_str(text));
    }
}

impl core::error::Error for ParseError
{
}

fn
read_u16(data: &[u8], off: usize) -> Result<u16, ParseError>
{
    let end: usize = off.checked_add(2).ok_or(ParseError::Truncated)?;
    let b: &[u8] = data.get(off..end).ok_or(ParseError::Truncated)?;

    return (Ok(u16::from_le_bytes([b[0], b[1]])));
}

fn
read_u32(data: &[u8], off: usize) -> Result<u32, ParseError>
{
    let end: usize = off.checked_add(4).ok_or(ParseError::Truncated)?;
    let b: &[u8] = data.get(off..end).ok_or(ParseError::Truncated)?;

    return (Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]])));
}

fn
read_u64(data: &[u8], off: usize) -> Result<u64, ParseError>
{
    let end: usize = off.checked_add(8).ok_or(ParseError::Truncated)?;
    let b: &[u8] = data.get(off..end).ok_or(ParseError::Truncated)?;

    return (Ok(u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])));
}

fn
to_usize(value: u64) -> Result<usize, ParseError>
{
    return (usize::try_from(value).map_err(|_: core::num::TryFromIntError| -> ParseError
    {
        return (ParseError::Truncated);
    }));
}

fn
parse_interpreter(data: &[u8], offset: u64, filesz: u64) -> Result<String, ParseError>
{
    let start: usize = to_usize(offset)?;
    let end: usize = start.checked_add(to_usize(filesz)?).ok_or(ParseError::Truncated)?;
    let bytes: &[u8] = data.get(start..end).ok_or(ParseError::Truncated)?;
    let nul: usize = bytes.iter().position(|b: &u8| -> bool { return (*b == 0); }).unwrap_or(bytes.len());

    return (Ok(String::from_utf8_lossy(&bytes[..nul]).into_owned()));
}

/// Parses the ELF header and program headers.
pub fn
parse(data: &[u8]) -> Result<Elf, ParseError>
{
    if data.len() < 4
    {
        return (Err(ParseError::Truncated));
    }
    if &data[0..4] != b"\x7fELF"
    {
        return (Err(ParseError::BadMagic));
    }
    if data.len() < EHDR_SIZE
    {
        return (Err(ParseError::Truncated));
    }
    if data[4] != 2
    {
        return (Err(ParseError::UnsupportedClass));
    }
    if data[5] != 1
    {
        return (Err(ParseError::UnsupportedEndianness));
    }

    let kind: u16 = read_u16(data, 16)?;
    let machine: u16 = read_u16(data, 18)?;
    let entry: u64 = read_u64(data, 24)?;
    let phoff: usize = to_usize(read_u64(data, 32)?)?;
    let phentsize: usize = read_u16(data, 54)? as usize;
    let phnum: usize = read_u16(data, 56)? as usize;

    if phnum > 0 && phentsize < PHDR_SIZE
    {
        return (Err(ParseError::BadProgramHeader));
    }

    let mut segments: Vec<Segment> = Vec::new();
    let mut interpreter: Option<String> = None;
    let mut has_dynamic: bool = false;

    for index in 0..phnum
    {
        let base: usize = index
            .checked_mul(phentsize)
            .and_then(|delta: usize| -> Option<usize> { return (phoff.checked_add(delta)); })
            .ok_or(ParseError::Truncated)?;
        let p_type: u32 = read_u32(data, base)?;
        let flags: u32 = read_u32(data, base + 4)?;
        let offset: u64 = read_u64(data, base + 8)?;
        let vaddr: u64 = read_u64(data, base + 16)?;
        let filesz: u64 = read_u64(data, base + 32)?;
        let memsz: u64 = read_u64(data, base + 40)?;
        let align: u64 = read_u64(data, base + 48)?;

        match p_type
        {
            PT_LOAD =>
            {
                segments.push(Segment
                {
                    flags: flags,
                    offset: offset,
                    vaddr: vaddr,
                    filesz: filesz,
                    memsz: memsz,
                    align: align,
                });
            }
            PT_INTERP =>
            {
                interpreter = Some(parse_interpreter(data, offset, filesz)?);
            }
            PT_DYNAMIC =>
            {
                has_dynamic = true;
            }
            _ =>
            {
            }
        }
    }

    let elf: Elf = Elf
    {
        kind: kind,
        machine: machine,
        entry: entry,
        interpreter: interpreter,
        has_dynamic: has_dynamic,
        segments: segments,
    };

    return (Ok(elf));
}

/// Why a parsed ELF file is not a loadable LIU kernel image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageError
{
    Parse(ParseError),
    WrongMachine(u16),
    WrongType(u16),
    HasInterpreter,
    NoLoadSegments,
    /// Segment `index` has `filesz > memsz`.
    FileLargerThanMemory(usize),
    /// Segment `index` extends past the end of the file.
    OutsideFile(usize),
    /// Segment `index` has an invalid alignment or `vaddr`/`offset` mismatch.
    BadAlignment(usize),
    /// Segment `index` overlaps or precedes the previous one.
    Overlap(usize),
    /// Segment `index` is both writable and executable.
    WritableAndExecutable(usize),
    /// The entry point is not inside an executable segment.
    EntryOutsideCode,
    AddressOverflow,
}

impl fmt::Display for ImageError
{
    fn
    fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
        let result: fmt::Result = match self
        {
            ImageError::Parse(error) => write!(f, "{}", error),
            ImageError::WrongMachine(machine) => write!(f, "unsupported machine {}", machine),
            ImageError::WrongType(kind) => write!(f, "unsupported ELF type {}", kind),
            ImageError::HasInterpreter => f.write_str("kernel image requests an interpreter"),
            ImageError::NoLoadSegments => f.write_str("no loadable segments"),
            ImageError::FileLargerThanMemory(i) => write!(f, "segment {}: filesz > memsz", i),
            ImageError::OutsideFile(i) => write!(f, "segment {}: outside the file", i),
            ImageError::BadAlignment(i) => write!(f, "segment {}: bad alignment", i),
            ImageError::Overlap(i) => write!(f, "segment {}: overlaps or is out of order", i),
            ImageError::WritableAndExecutable(i) => write!(f, "segment {}: writable and executable", i),
            ImageError::EntryOutsideCode => f.write_str("entry point outside executable code"),
            ImageError::AddressOverflow => f.write_str("address arithmetic overflows"),
        };

        return (result);
    }
}

impl core::error::Error for ImageError
{
}

impl From<ParseError> for ImageError
{
    fn
    from(error: ParseError) -> ImageError
    {
        return (ImageError::Parse(error));
    }
}

/// Where and how to load a kernel image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadPlan
{
    /// Page-aligned lowest address of the image (link-time address).
    pub base: u64,
    /// Page-aligned size of the address range the image occupies.
    pub size: u64,
    /// Entry point (link-time address).
    pub entry: u64,
    /// Whether the image is position-independent and may be relocated.
    pub relocatable: bool,
    pub segments: Vec<Segment>,
}

fn
align_down(value: u64) -> u64
{
    return (value & !(PAGE_SIZE - 1));
}

fn
align_up(value: u64) -> Option<u64>
{
    return (value.checked_add(PAGE_SIZE - 1).map(|v: u64| -> u64 { return (align_down(v)); }));
}

fn
check_segment(index: usize, seg: &Segment, file_len: u64) -> Result<u64, ImageError>
{
    if seg.filesz > seg.memsz
    {
        return (Err(ImageError::FileLargerThanMemory(index)));
    }

    let file_end: u64 = seg.offset.checked_add(seg.filesz).ok_or(ImageError::AddressOverflow)?;

    if file_end > file_len
    {
        return (Err(ImageError::OutsideFile(index)));
    }
    if seg.align > 1 && (!seg.align.is_power_of_two() || seg.vaddr % seg.align != seg.offset % seg.align)
    {
        return (Err(ImageError::BadAlignment(index)));
    }
    if seg.is_write() && seg.is_exec()
    {
        return (Err(ImageError::WritableAndExecutable(index)));
    }

    let end: u64 = seg.vaddr.checked_add(seg.memsz).ok_or(ImageError::AddressOverflow)?;

    return (Ok(end));
}

/// Validates `data` as a LIU kernel image for `machine` and plans its load.
///
/// A kernel image is a static `ET_EXEC` or `ET_DYN` file without an
/// interpreter, whose `PT_LOAD` segments are sorted, non-overlapping, inside
/// the file, never writable and executable at once, and whose entry point
/// (`liu_kernel_entry`) lies in an executable segment.
pub fn
plan_kernel(data: &[u8], machine: u16) -> Result<LoadPlan, ImageError>
{
    let elf: Elf = parse(data)?;

    if elf.machine != machine
    {
        return (Err(ImageError::WrongMachine(elf.machine)));
    }
    if elf.kind != ET_EXEC && elf.kind != ET_DYN
    {
        return (Err(ImageError::WrongType(elf.kind)));
    }
    if elf.interpreter.is_some()
    {
        return (Err(ImageError::HasInterpreter));
    }

    let segments: Vec<Segment> = elf
        .segments
        .into_iter()
        .filter(|seg: &Segment| -> bool { return (seg.memsz > 0); })
        .collect();

    if segments.is_empty()
    {
        return (Err(ImageError::NoLoadSegments));
    }

    let file_len: u64 = data.len() as u64;
    let mut previous_end: u64 = 0;
    let mut entry_ok: bool = false;

    for (index, seg) in segments.iter().enumerate()
    {
        let end: u64 = check_segment(index, seg, file_len)?;

        if index > 0 && seg.vaddr < previous_end
        {
            return (Err(ImageError::Overlap(index)));
        }
        if seg.is_exec() && elf.entry >= seg.vaddr && elf.entry < end
        {
            entry_ok = true;
        }
        previous_end = end;
    }

    if !entry_ok
    {
        return (Err(ImageError::EntryOutsideCode));
    }

    let base: u64 = align_down(segments[0].vaddr);
    let top: u64 = align_up(previous_end).ok_or(ImageError::AddressOverflow)?;
    let plan: LoadPlan = LoadPlan
    {
        base: base,
        size: top - base,
        entry: elf.entry,
        relocatable: elf.kind == ET_DYN,
        segments: segments,
    };

    return (Ok(plan));
}

#[cfg(test)]
mod tests
{
    use super::*;
    use std::vec;

    fn
    put_u16(buf: &mut [u8], off: usize, v: u16) -> ()
    {
        buf[off..off + 2].copy_from_slice(&v.to_le_bytes());
    }

    fn
    put_u32(buf: &mut [u8], off: usize, v: u32) -> ()
    {
        buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn
    put_u64(buf: &mut [u8], off: usize, v: u64) -> ()
    {
        buf[off..off + 8].copy_from_slice(&v.to_le_bytes());
    }

    /// `(flags, offset, vaddr, filesz, memsz)` for each PT_LOAD.
    fn
    image(kind: u16, entry: u64, loads: &[(u32, u64, u64, u64, u64)]) -> Vec<u8>
    {
        let mut b: Vec<u8> = vec![0u8; 0x3000];

        b[0..4].copy_from_slice(b"\x7fELF");
        b[4] = 2;
        b[5] = 1;
        put_u16(&mut b, 16, kind);
        put_u16(&mut b, 18, EM_X86_64);
        put_u64(&mut b, 24, entry);
        put_u64(&mut b, 32, 64);
        put_u16(&mut b, 54, PHDR_SIZE as u16);
        put_u16(&mut b, 56, loads.len() as u16);

        for (i, (flags, offset, vaddr, filesz, memsz)) in loads.iter().enumerate()
        {
            let ph: usize = 64 + i * PHDR_SIZE;

            put_u32(&mut b, ph, PT_LOAD);
            put_u32(&mut b, ph + 4, *flags);
            put_u64(&mut b, ph + 8, *offset);
            put_u64(&mut b, ph + 16, *vaddr);
            put_u64(&mut b, ph + 32, *filesz);
            put_u64(&mut b, ph + 40, *memsz);
            put_u64(&mut b, ph + 48, PAGE_SIZE);
        }

        return (b);
    }

    fn
    good() -> Vec<u8>
    {
        return (image(
            ET_EXEC,
            0x40_1000,
            &[(PF_R | PF_X, 0x1000, 0x40_1000, 0x800, 0x800), (PF_R | PF_W, 0x2000, 0x40_2000, 0x100, 0x2000)],
        ));
    }

    #[test]
    fn
    parses_header_and_segments() -> ()
    {
        let elf: Elf = parse(&good()).unwrap();

        assert_eq!(elf.kind, ET_EXEC);
        assert_eq!(elf.machine, EM_X86_64);
        assert_eq!(elf.entry, 0x40_1000);
        assert_eq!(elf.segments.len(), 2);
        assert_eq!(elf.segments[1].memsz, 0x2000);
    }

    #[test]
    fn
    rejects_bad_magic_and_truncation() -> ()
    {
        assert_eq!(parse(&[0u8; 64]), Err(ParseError::BadMagic));
        assert_eq!(parse(&[0u8; 2]), Err(ParseError::Truncated));
        assert_eq!(parse(&good()[..40]), Err(ParseError::Truncated));
    }

    #[test]
    fn
    plans_a_valid_kernel() -> ()
    {
        let plan: LoadPlan = plan_kernel(&good(), EM_X86_64).unwrap();

        assert_eq!(plan.base, 0x40_1000);
        assert_eq!(plan.size, 0x3000);
        assert_eq!(plan.entry, 0x40_1000);
        assert!(!plan.relocatable);
    }

    #[test]
    fn
    rejects_wrong_machine() -> ()
    {
        assert_eq!(plan_kernel(&good(), EM_AARCH64), Err(ImageError::WrongMachine(EM_X86_64)));
    }

    #[test]
    fn
    rejects_entry_outside_code() -> ()
    {
        let data: Vec<u8> = image(ET_EXEC, 0x40_2000, &[
            (PF_R | PF_X, 0x1000, 0x40_1000, 0x800, 0x800),
            (PF_R | PF_W, 0x2000, 0x40_2000, 0x100, 0x100),
        ]);

        assert_eq!(plan_kernel(&data, EM_X86_64), Err(ImageError::EntryOutsideCode));
    }

    #[test]
    fn
    rejects_overlap() -> ()
    {
        let data: Vec<u8> = image(ET_EXEC, 0x40_1000, &[
            (PF_R | PF_X, 0x1000, 0x40_1000, 0x800, 0x1800),
            (PF_R, 0x2000, 0x40_2000, 0x100, 0x100),
        ]);

        assert_eq!(plan_kernel(&data, EM_X86_64), Err(ImageError::Overlap(1)));
    }

    #[test]
    fn
    rejects_segment_outside_file() -> ()
    {
        let data: Vec<u8> = image(ET_EXEC, 0x40_1000, &[(PF_R | PF_X, 0x1000, 0x40_1000, 0x9000, 0x9000)]);

        assert_eq!(plan_kernel(&data, EM_X86_64), Err(ImageError::OutsideFile(0)));
    }

    #[test]
    fn
    rejects_writable_executable() -> ()
    {
        let data: Vec<u8> = image(ET_EXEC, 0x40_1000, &[(PF_R | PF_W | PF_X, 0x1000, 0x40_1000, 0x10, 0x10)]);

        assert_eq!(plan_kernel(&data, EM_X86_64), Err(ImageError::WritableAndExecutable(0)));
    }

    #[test]
    fn
    rejects_filesz_larger_than_memsz() -> ()
    {
        let data: Vec<u8> = image(ET_EXEC, 0x40_1000, &[(PF_R | PF_X, 0x1000, 0x40_1000, 0x20, 0x10)]);

        assert_eq!(plan_kernel(&data, EM_X86_64), Err(ImageError::FileLargerThanMemory(0)));
    }

    #[test]
    fn
    marks_pie_as_relocatable() -> ()
    {
        let data: Vec<u8> = image(ET_DYN, 0x1000, &[(PF_R | PF_X, 0x1000, 0x1000, 0x10, 0x10)]);

        assert!(plan_kernel(&data, EM_X86_64).unwrap().relocatable);
    }
}
