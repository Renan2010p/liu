// SPDX-License-Identifier: MPL-2.0

//! Generation of the C host ABI header from liu-abi.

use std::fs;
use std::path::{Path, PathBuf};

use crate::TaskResult;

/// Location of the generated header, relative to the workspace root.
pub const HEADER: &str = "kernel/os-liu/include/liu/host.h";

/// Directory added to the include path to find `liu/host.h`.
pub const INCLUDE_DIR: &str = "kernel/os-liu/include";

fn
render(root: &Path) -> Result<Vec<u8>, String>
{
    let crate_dir: PathBuf = root.join("crates/liu-abi");
    let config: cbindgen::Config = cbindgen::Config::from_file(crate_dir.join("cbindgen.toml"))
        .map_err(|error: String| -> String
        {
            return (format!("cbindgen.toml: {}", error));
        })?;
    let bindings: cbindgen::Bindings = cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(config)
        .generate()
        .map_err(|error: cbindgen::Error| -> String
        {
            return (format!("cbindgen: {}", error));
        })?;
    let mut out: Vec<u8> = Vec::new();

    bindings.write(&mut out);

    return (Ok(out));
}

/// Writes the header.
pub fn
generate_file(root: &Path) -> TaskResult
{
    let content: Vec<u8> = render(root)?;
    let path: PathBuf = root.join(HEADER);

    if let Some(dir) = path.parent()
    {
        fs::create_dir_all(dir).map_err(|error: std::io::Error| -> String
        {
            return (format!("{}: {}", dir.display(), error));
        })?;
    }
    fs::write(&path, content).map_err(|error: std::io::Error| -> String
    {
        return (format!("{}: {}", path.display(), error));
    })?;
    println!("wrote {}", HEADER);

    return (Ok(()));
}

/// Fails if the committed header differs from a fresh rendering.
pub fn
check(root: &Path) -> TaskResult
{
    let expected: Vec<u8> = render(root)?;
    let path: PathBuf = root.join(HEADER);
    let actual: Vec<u8> = fs::read(&path).map_err(|error: std::io::Error| -> String
    {
        return (format!("{}: {} (run `cargo xtask gen-header`)", HEADER, error));
    })?;

    if actual != expected
    {
        return (Err(format!("{} is out of date; run `cargo xtask gen-header`", HEADER)));
    }
    println!("check-header: {} is up to date", HEADER);

    return (Ok(()));
}
