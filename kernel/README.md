# kernel/

Linux-side code of LIU. Everything here is **GPL-2.0-only** unless a file says
otherwise.

- `os-liu/include/liu/host.h` — host ABI header, generated from `crates/liu-abi`
  by `cargo xtask gen-header` (dual MPL-2.0 OR GPL-2.0-only). Do not edit.

The patch series against the pinned Linux LTS tree and the `os-liu` host layer
arrive in milestone M3 (see `docs/architecture.md` §9 and §16).
