// SPDX-License-Identifier: MPL-2.0

//! Cross-checks the generated C header against the Rust layout.
//!
//! A C program is generated that includes `liu/host.h` and prints the size,
//! alignment and field offsets of every boundary type, plus the value of the
//! ABI constants. Its output must equal the same facts computed in Rust.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use liu_abi::layout::TYPES;

use crate::header::INCLUDE_DIR;
use crate::TaskResult;

/// `(C name, value)` of every constant the kernel side relies on.
fn
constants() -> Vec<(&'static str, i64)>
{
    let list: Vec<(&'static str, i64)> = vec![
        ("LIU_ABI_MAGIC", liu_abi::ABI_MAGIC as i64),
        ("LIU_ABI_VERSION_MAJOR", liu_abi::ABI_VERSION_MAJOR as i64),
        ("LIU_ABI_VERSION_MINOR", liu_abi::ABI_VERSION_MINOR as i64),
        ("LIU_STATUS_OK", liu_abi::status::STATUS_OK as i64),
        ("LIU_STATUS_NOT_FOUND", liu_abi::status::STATUS_NOT_FOUND as i64),
        ("LIU_TRAP_SYSCALL", liu_abi::trap::TRAP_SYSCALL as i64),
        ("LIU_TRAP_EXITED", liu_abi::trap::TRAP_EXITED as i64),
        ("LIU_PROT_EXEC", liu_abi::PROT_EXEC as i64),
        ("LIU_PAGE_SIZE", liu_abi::PAGE_SIZE as i64),
    ];

    return (list);
}

/// Appends `text` and a newline.
fn
line(out: &mut String, text: String) -> ()
{
    out.push_str(&text);
    out.push('\n');
}

fn
expected() -> String
{
    let mut out: String = String::new();

    for layout in TYPES
    {
        line(&mut out, format!("{} size {}", layout.c_name, layout.size));
        line(&mut out, format!("{} align {}", layout.c_name, layout.align));
        for (field, offset) in layout.fields
        {
            line(&mut out, format!("{}.{} {}", layout.c_name, field, offset));
        }
    }
    for (name, value) in constants()
    {
        line(&mut out, format!("{} {}", name, value));
    }

    return (out);
}

fn
probe_source() -> String
{
    let mut src: String = String::from(
        "#include <stdio.h>\n#include <stddef.h>\n#include \"liu/host.h\"\n\nint\nmain(void)\n{\n",
    );

    for layout in TYPES
    {
        let ty: String = format!("struct {}", layout.c_name);
        let c_name: &str = layout.c_name;
        let fields: &[(&str, usize)] = layout.fields;

        line(&mut src, format!("\tprintf(\"{} size %zu\\n\", sizeof({}));", c_name, ty));
        line(&mut src, format!("\tprintf(\"{} align %zu\\n\", _Alignof({}));", c_name, ty));
        for (field, _) in fields.iter()
        {
            line(&mut src, format!(
                "\tprintf(\"{}.{} %zu\\n\", offsetof({}, {}));",
                c_name, field, ty, field
            ));
        }
    }
    for (name, _) in constants()
    {
        line(&mut src, format!("\tprintf(\"{} %lld\\n\", (long long){});", name, name));
    }
    src.push_str("\treturn (0);\n}\n");

    return (src);
}

fn
io_error(path: &Path) -> impl Fn(std::io::Error) -> String + '_
{
    return (move |error: std::io::Error| -> String
    {
        return (format!("{}: {}", path.display(), error));
    });
}

/// Compiles and runs the probe, then compares its output.
pub fn
check(root: &Path) -> TaskResult
{
    let dir: PathBuf = root.join("target/xtask/abi");
    let source: PathBuf = dir.join("probe.c");
    let binary: PathBuf = dir.join("probe");
    let cc: String = env::var("CC").unwrap_or_else(|_: env::VarError| -> String
    {
        return (String::from("cc"));
    });

    fs::create_dir_all(&dir).map_err(io_error(&dir))?;
    fs::write(&source, probe_source()).map_err(io_error(&source))?;

    let include: PathBuf = root.join(INCLUDE_DIR);
    let compile: Output = Command::new(&cc)
        .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-pedantic"])
        .arg("-I")
        .arg(&include)
        .arg("-o")
        .arg(&binary)
        .arg(&source)
        .output()
        .map_err(|error: std::io::Error| -> String
        {
            return (format!("cannot run {}: {}", cc, error));
        })?;

    if !compile.status.success()
    {
        return (Err(format!(
            "host.h does not compile as C11:\n{}",
            String::from_utf8_lossy(&compile.stderr)
        )));
    }

    let run: Output = Command::new(&binary).output().map_err(io_error(&binary))?;
    let actual: String = String::from_utf8_lossy(&run.stdout).into_owned();
    let expected: String = expected();

    if actual != expected
    {
        let mut report: String = String::from("C and Rust layouts differ:\n");

        for (c_line, rust_line) in actual.lines().zip(expected.lines())
        {
            if c_line != rust_line
            {
                line(&mut report, format!("  C:    {}\n  Rust: {}", c_line, rust_line));
            }
        }
        return (Err(report));
    }
    println!("check-abi: {} types and {} constants match", TYPES.len(), constants().len());

    return (Ok(()));
}
