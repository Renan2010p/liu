// SPDX-License-Identifier: MPL-2.0

//! The ABI test kernel (`tests/abi-kernel`): build it and run it under the
//! runtime.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, ExitStatus, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::TaskResult;

/// Target the test kernel is built for: no host OS, no libc.
pub const TARGET: &str = "x86_64-unknown-none";

/// Static, non-PIE code at the link address in `link.ld`. The small code
/// model fits because the image lives below 2 GiB, like arch/um.
const RUSTFLAGS: [&str; 2] = ["-Crelocation-model=static", "-Ccode-model=small"];

/// Line the test kernel prints last when every check passed.
const PASS_LINE: &str = "abi-kernel: pass";

/// A kernel blocked in `idle` with nothing armed never returns: give up
/// after this long instead of hanging CI.
const RUN_TIMEOUT: Duration = Duration::from_secs(60);

/// Runs `command`, killing it after [`RUN_TIMEOUT`]. Returns its exit status
/// and standard output; standard error is inherited.
fn
run_with_timeout(command: &mut Command) -> Result<(ExitStatus, String), String>
{
    let mut child: Child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error: std::io::Error| -> String
        {
            return (format!("cannot run liu-kernel: {}", error));
        })?;
    let mut stdout: ChildStdout = child.stdout.take().ok_or(String::from("no stdout pipe"))?;
    let reader: thread::JoinHandle<String> = thread::spawn(move || -> String
    {
        let mut text: String = String::new();
        let _: std::io::Result<usize> = stdout.read_to_string(&mut text);

        return (text);
    });
    let start: Instant = Instant::now();

    let status: ExitStatus = loop
    {
        match child.try_wait()
        {
            Ok(Some(status)) =>
            {
                break status;
            }
            Ok(None) if start.elapsed() < RUN_TIMEOUT =>
            {
                thread::sleep(Duration::from_millis(20));
            }
            Ok(None) =>
            {
                let _: std::io::Result<()> = child.kill();
                let _: std::io::Result<ExitStatus> = child.wait();
                let text: String = reader.join().unwrap_or_default();

                print!("{}", text);
                return (Err(format!("liu-kernel did not finish within {:?}", RUN_TIMEOUT)));
            }
            Err(error) =>
            {
                return (Err(format!("cannot wait for liu-kernel: {}", error)));
            }
        }
    };
    let text: String = reader.join().unwrap_or_default();

    return (Ok((status, text)));
}

/// Path of the built test kernel image.
pub fn
image(root: &Path) -> PathBuf
{
    return (root.join("target/abi-kernel").join(TARGET).join("release/abi-kernel"));
}

/// Runs a cargo subcommand (`build`, `clippy`) on the test kernel; `extra`
/// is passed after `--`.
fn
cargo(root: &Path, subcommand: &str, extra: &[&str]) -> TaskResult
{
    let manifest: PathBuf = root.join("tests/abi-kernel/Cargo.toml");
    let target_dir: PathBuf = root.join("target/abi-kernel");

    println!("+ cargo {} --release --target {} --manifest-path {}", subcommand, TARGET, manifest.display());

    let mut command: Command = Command::new("cargo");

    command
        .args([subcommand, "--quiet", "--release", "--target", TARGET])
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--target-dir")
        .arg(&target_dir);
    if !extra.is_empty()
    {
        command.arg("--").args(extra);
    }

    let output: Output = command
        .env_remove("RUSTFLAGS")
        .env("CARGO_ENCODED_RUSTFLAGS", RUSTFLAGS.join("\x1f"))
        .current_dir(root)
        .output()
        .map_err(|error: std::io::Error| -> String
        {
            return (format!("cannot run cargo: {}", error));
        })?;

    let stderr: String = String::from_utf8_lossy(&output.stderr).into_owned();

    eprint!("{}", stderr);
    if !output.status.success()
    {
        let mut message: String = format!("cargo {} on the ABI test kernel failed", subcommand);

        if stderr.contains("target may not be installed") || stderr.contains("can't find crate for `core`")
        {
            message.push_str(&format!("; run `rustup target add {}`", TARGET));
        }
        return (Err(message));
    }

    return (Ok(()));
}

/// Builds the test kernel.
pub fn
build(root: &Path) -> TaskResult
{
    return (cargo(root, "build", &[]));
}

/// Lints the test kernel, which is outside the workspace clippy run.
pub fn
clippy(root: &Path) -> TaskResult
{
    return (cargo(root, "clippy", &["-D", "warnings"]));
}

/// Builds the runtime and the test kernel, runs the kernel, and checks that
/// it reported success.
pub fn
test(root: &Path) -> TaskResult
{
    build(root)?;
    crate::run("cargo", &["build", "--quiet", "-p", "liu-runtime", "--bin", "liu-kernel"])?;

    let runtime: PathBuf = root.join("target/debug/liu-kernel");
    let kernel: PathBuf = image(root);
    let cmdline: &str = "console=hvc0 liu.test=abi";

    println!("+ {} --kernel {} --cmdline \"{}\"", runtime.display(), kernel.display(), cmdline);

    let mut command: Command = Command::new(&runtime);

    command
        .arg("--kernel")
        .arg(&kernel)
        .args(["--memory", "32M", "--cmdline", cmdline])
        .current_dir(root);

    let (status, stdout): (ExitStatus, String) = run_with_timeout(&mut command)?;

    print!("{}", stdout);
    if !status.success()
    {
        return (Err(format!("liu-kernel failed: {}", status)));
    }
    if stdout.lines().last() != Some(PASS_LINE)
    {
        return (Err(format!("the test kernel did not print \"{}\"", PASS_LINE)));
    }
    if !stdout.contains(&format!("cmdline \"{}\"", cmdline))
    {
        return (Err(String::from("the kernel did not receive the command line")));
    }

    println!("test-abi: passed");
    return (Ok(()));
}
