// SPDX-License-Identifier: MPL-2.0

//! Links the test kernel with its own linker script.

use std::env;

fn
main() -> ()
{
    let dir: String = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_: env::VarError| -> String
    {
        return (String::from("."));
    });

    println!("cargo:rerun-if-changed=link.ld");
    println!("cargo:rustc-link-arg-bins=-T{}/link.ld", dir);
    println!("cargo:rustc-link-arg-bins=--no-pie");
    println!("cargo:rustc-link-arg-bins=-zmax-page-size=4096");
}
