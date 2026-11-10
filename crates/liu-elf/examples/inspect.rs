// SPDX-License-Identifier: MPL-2.0

//! Prints the ELF header and load segments of a file, and whether it is a
//! valid LIU kernel image.
//!
//!     cargo run -p liu-elf --example inspect -- <file>

use std::env;
use std::fs;
use std::process::ExitCode;

use liu_elf::{Elf, LoadPlan, ImageError, EM_X86_64};

fn
main() -> ExitCode
{
    let args: Vec<String> = env::args().collect();

    if args.len() != 2
    {
        eprintln!("usage: inspect <elf-file>");
        return (ExitCode::from(2));
    }

    let data: Vec<u8> = match fs::read(&args[1])
    {
        Ok(data) => data,
        Err(error) =>
        {
            eprintln!("{}: {}", args[1], error);
            return (ExitCode::FAILURE);
        }
    };
    let elf: Elf = match liu_elf::parse(&data)
    {
        Ok(elf) => elf,
        Err(error) =>
        {
            eprintln!("{}: {}", args[1], error);
            return (ExitCode::FAILURE);
        }
    };

    println!("type:        {}", elf.kind);
    println!("machine:     {}", elf.machine);
    println!("entry:       {:#x}", elf.entry);
    if let Some(interp) = &elf.interpreter
    {
        println!("interpreter: {}", interp);
    }
    for seg in &elf.segments
    {
        println!(
            "LOAD  vaddr={:#014x} memsz={:#010x} filesz={:#010x} flags={}{}{}",
            seg.vaddr,
            seg.memsz,
            seg.filesz,
            if seg.flags & liu_elf::PF_R != 0 { "r" } else { "-" },
            if seg.is_write() { "w" } else { "-" },
            if seg.is_exec() { "x" } else { "-" },
        );
    }

    let result: Result<LoadPlan, ImageError> = liu_elf::plan_kernel(&data, EM_X86_64);

    match result
    {
        Ok(plan) =>
        {
            println!("kernel image: ok (base={:#x} size={:#x})", plan.base, plan.size);
        }
        Err(error) =>
        {
            println!("kernel image: no ({})", error);
        }
    }

    return (ExitCode::SUCCESS);
}
