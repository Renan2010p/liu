# Contributing to LIU

## Before you start

Read [`docs/architecture.md`](docs/architecture.md) and the
[ADRs](docs/adr/). Significant design changes need a new ADR.

## Building and checking

```sh
cargo build --workspace
cargo xtask ci          # style, clippy, tests, header, C ABI, cross-check
```

`cargo xtask ci` must pass before a change is merged. On FreeBSD and Renux,
the `liu-platform` tests exercise the running kernel (ptrace, Capsicum, shared
memory). The Renux `PT_SET_SC_RET` tests skip on a stock FreeBSD kernel.

To type-check the Linux code from a FreeBSD host:

```sh
rustup target add x86_64-unknown-linux-musl
```

## Rules

- Rust code follows KNF: see [`docs/style.md`](docs/style.md). Do not run
  `cargo fmt`.
- Host-specific code belongs in `liu-platform` only.
- Never edit `kernel/os-liu/include/liu/host.h` by hand. Change `liu-abi` and
  run `cargo xtask gen-header`.
- Any change to a `#[repr(C)]` type in `liu-abi` is an ABI change: append new
  table entries at the end and bump `ABI_VERSION_MINOR`; bump
  `ABI_VERSION_MAJOR` for anything else.
- Renux kernel changes needed by LIU must be generic (ADR 0005).

## Licensing

By contributing you agree that your contribution is licensed under the
Mozilla Public License 2.0. Files under `kernel/` that are derived from Linux
are GPL-2.0-only.
