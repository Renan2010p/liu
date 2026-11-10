// SPDX-License-Identifier: MPL-2.0

//! LIU maintenance tasks.
//!
//!     cargo xtask gen-header     regenerate kernel/os-liu/include/liu/host.h
//!     cargo xtask check-header   fail if host.h is out of date
//!     cargo xtask check-abi      compare the C layout of host.h with liu-abi
//!     cargo xtask style          check the KNF style rules (docs/style.md)
//!     cargo xtask ci             run every check the CI runs

mod abi;
mod header;
mod style;

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, ExitStatus};

/// Result type of every task: an error is a message for the user.
pub type TaskResult = Result<(), String>;

/// Workspace root.
pub fn
root() -> PathBuf
{
    let manifest: &Path = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root: PathBuf = match manifest.parent()
    {
        Some(parent) => parent.to_path_buf(),
        None => manifest.to_path_buf(),
    };

    return (root);
}

/// Runs a command in the workspace root and fails if it fails.
pub fn
run(program: &str, args: &[&str]) -> TaskResult
{
    println!("+ {} {}", program, args.join(" "));

    let status: ExitStatus = Command::new(program)
        .args(args)
        .current_dir(root())
        .status()
        .map_err(|error: std::io::Error| -> String
        {
            return (format!("cannot run {}: {}", program, error));
        })?;

    if !status.success()
    {
        return (Err(format!("{} {} failed: {}", program, args.join(" "), status)));
    }

    return (Ok(()));
}

/// Returns whether a rustup target is installed.
fn
target_installed(target: &str) -> bool
{
    let output: Result<std::process::Output, std::io::Error> =
        Command::new("rustup").args(["target", "list", "--installed"]).output();

    let installed: bool = match output
    {
        Ok(output) => String::from_utf8_lossy(&output.stdout).lines().any(|line: &str| -> bool
        {
            return (line.trim() == target);
        }),
        Err(_) => false,
    };

    return (installed);
}

/// Type-checks the portable crates for the other Tier 1 hosts.
fn
cross_check() -> TaskResult
{
    let targets: [&str; 2] = ["x86_64-unknown-linux-musl", "x86_64-unknown-linux-gnu"];

    for target in targets
    {
        if !target_installed(target)
        {
            println!("skip: rust target {} not installed", target);
            continue;
        }
        run("cargo", &["check", "--quiet", "-p", "liu-platform", "-p", "liu-abi", "-p", "liu-elf",
            "--all-targets", "--target", target])?;

        return (Ok(()));
    }

    println!("skip: no Linux rust target installed; run `rustup target add x86_64-unknown-linux-musl`");
    return (Ok(()));
}

fn
ci() -> TaskResult
{
    style::check(&root())?;
    run("cargo", &["clippy", "--quiet", "--workspace", "--all-targets", "--", "-D", "warnings"])?;
    run("cargo", &["test", "--quiet", "--workspace"])?;
    header::check(&root())?;
    abi::check(&root())?;
    cross_check()?;
    println!("ci: all checks passed");

    return (Ok(()));
}

fn
usage() -> TaskResult
{
    return (Err(String::from(
        "usage: cargo xtask <gen-header | check-header | check-abi | style | ci>",
    )));
}

fn
main() -> ExitCode
{
    let args: Vec<String> = env::args().skip(1).collect();
    let task: &str = match args.first()
    {
        Some(task) => task.as_str(),
        None => "",
    };
    let result: TaskResult = match task
    {
        "gen-header" => header::generate_file(&root()),
        "check-header" => header::check(&root()),
        "check-abi" => abi::check(&root()),
        "style" => style::check(&root()),
        "ci" => ci(),
        _ => usage(),
    };

    if let Err(message) = result
    {
        eprintln!("error: {}", message);
        return (ExitCode::FAILURE);
    }

    return (ExitCode::SUCCESS);
}
