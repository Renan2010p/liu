// SPDX-License-Identifier: MPL-2.0

//! Loads a freestanding kernel image at its link-time address.

use std::fmt;
use std::io;

use liu_elf::{ImageError, LoadPlan, Segment, EM_X86_64};
use liu_platform::{Protection, Region};

use liu_abi::PAGE_SIZE;

/// Why an image could not be loaded.
#[derive(Debug)]
pub enum LoadError
{
    /// The file is not a valid LIU kernel image.
    Image(ImageError),
    /// Position-independent images need relocation, which is not supported.
    Relocatable,
    /// Segment `index` shares a page with the previous one but asks for
    /// different permissions.
    SharedPage(usize),
    /// The image range could not be mapped (usually: already in use).
    Map(u64, io::Error),
    Protect(io::Error),
}

impl fmt::Display for LoadError
{
    fn
    fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
        let result: fmt::Result = match self
        {
            LoadError::Image(error) => write!(f, "{}", error),
            LoadError::Relocatable => f.write_str("relocatable (ET_DYN) images are not supported"),
            LoadError::SharedPage(i) =>
            {
                write!(f, "segment {}: shares a page with a segment of other permissions", i)
            }
            LoadError::Map(addr, error) => write!(f, "cannot map the image at {:#x}: {}", addr, error),
            LoadError::Protect(error) => write!(f, "cannot protect the image: {}", error),
        };

        return (result);
    }
}

impl std::error::Error for LoadError
{
}

impl From<ImageError> for LoadError
{
    fn
    from(error: ImageError) -> LoadError
    {
        return (LoadError::Image(error));
    }
}

fn
page_down(value: u64) -> u64
{
    return (value & !(PAGE_SIZE - 1));
}

fn
page_up(value: u64) -> u64
{
    return (page_down(value + PAGE_SIZE - 1));
}

fn
protection(seg: &Segment) -> Protection
{
    let mut prot: Protection = Protection::NONE;

    if seg.flags & liu_elf::PF_R != 0
    {
        prot = prot.union(Protection::READ);
    }
    if seg.is_write()
    {
        prot = prot.union(Protection::WRITE);
    }
    if seg.is_exec()
    {
        prot = prot.union(Protection::EXEC);
    }

    return (prot);
}

/// Refuses segments whose permissions would have to be merged on a shared
/// page: merging could make a page both writable and executable.
fn
check_pages(plan: &LoadPlan) -> Result<(), LoadError>
{
    for index in 1..plan.segments.len()
    {
        let prev: &Segment = &plan.segments[index - 1];
        let seg: &Segment = &plan.segments[index];

        if page_down(seg.vaddr) < page_up(prev.vaddr + prev.memsz) && seg.flags != prev.flags
        {
            return (Err(LoadError::SharedPage(index)));
        }
    }

    return (Ok(()));
}

/// Applies `prot` to the image pages `[from, to)`; `base` is the image base.
fn
protect(region: &mut Region, base: u64, from: u64, to: u64, prot: Protection) -> Result<(), LoadError>
{
    let result: io::Result<()> = region.protect((from - base) as usize, (to - from) as usize, prot);

    return (result.map_err(LoadError::Protect));
}

/// A kernel image mapped at its link-time address.
///
/// The entry point is guaranteed to lie in an executable segment of the
/// mapped image, which is what makes [`crate::ops::enter`] sound.
#[derive(Debug)]
pub struct KernelImage
{
    region: Region,
    entry: u64,
}

impl KernelImage
{
    /// Validates `data`, maps it and applies each segment's permissions.
    pub fn
    load(data: &[u8]) -> Result<KernelImage, LoadError>
    {
        let plan: LoadPlan = liu_elf::plan_kernel(data, EM_X86_64)?;

        if plan.relocatable
        {
            return (Err(LoadError::Relocatable));
        }
        check_pages(&plan)?;

        let mut region: Region = Region::reserve_at(plan.base, plan.size as usize)
            .map_err(|error: io::Error| -> LoadError { return (LoadError::Map(plan.base, error)); })?;

        for seg in &plan.segments
        {
            let start: usize = seg.offset as usize;
            let bytes: &[u8] = &data[start..start + seg.filesz as usize];

            region.write_at((seg.vaddr - plan.base) as usize, bytes).map_err(
                |error: io::Error| -> LoadError { return (LoadError::Map(plan.base, error)); },
            )?;
        }

        protect(&mut region, plan.base, plan.base, plan.base + plan.size, Protection::NONE)?;
        for seg in &plan.segments
        {
            let (from, to): (u64, u64) = (page_down(seg.vaddr), page_up(seg.vaddr + seg.memsz));

            protect(&mut region, plan.base, from, to, protection(seg))?;
        }

        return (Ok(KernelImage { region: region, entry: plan.entry }));
    }

    /// Lowest address of the image.
    pub fn
    base(&self) -> u64
    {
        return (self.region.addr());
    }

    /// Page-aligned size of the image.
    pub fn
    size(&self) -> u64
    {
        return (self.region.len() as u64);
    }

    /// Address of `liu_kernel_entry`.
    pub fn
    entry(&self) -> u64
    {
        return (self.entry);
    }
}

#[cfg(test)]
mod tests
{
    use super::*;

    fn
    seg(flags: u32, vaddr: u64, memsz: u64) -> Segment
    {
        return (Segment { flags: flags, offset: 0, vaddr: vaddr, filesz: 0, memsz: memsz, align: 4096 });
    }

    fn
    plan(segments: Vec<Segment>) -> LoadPlan
    {
        return (LoadPlan { base: 0x1000, size: 0x4000, entry: 0x1000, relocatable: false, segments: segments });
    }

    #[test]
    fn
    accepts_page_separated_segments() -> ()
    {
        let r: u32 = liu_elf::PF_R;
        let rw: u32 = liu_elf::PF_R | liu_elf::PF_W;

        assert!(check_pages(&plan(vec![seg(r, 0x1000, 0x10), seg(rw, 0x2000, 0x10)])).is_ok());
    }

    #[test]
    fn
    accepts_shared_page_with_equal_flags() -> ()
    {
        let rw: u32 = liu_elf::PF_R | liu_elf::PF_W;

        assert!(check_pages(&plan(vec![seg(rw, 0x1000, 0x10), seg(rw, 0x1010, 0x10)])).is_ok());
    }

    #[test]
    fn
    refuses_shared_page_with_other_flags() -> ()
    {
        let rx: u32 = liu_elf::PF_R | liu_elf::PF_X;
        let rw: u32 = liu_elf::PF_R | liu_elf::PF_W;
        let result: Result<(), LoadError> =
            check_pages(&plan(vec![seg(rx, 0x1000, 0x10), seg(rw, 0x1010, 0x10)]));

        assert!(matches!(result, Err(LoadError::SharedPage(1))));
    }
}
