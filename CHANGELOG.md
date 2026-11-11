# Changelog

All notable changes to LIU are documented here.

## Unreleased

### Added

- Architecture document, ADRs 0001–0006, KNF style guide.
- `liu-abi`: host ABI v0.1 (`HostOps`, `KernelOps`, `BootInfo`, `GuestRegs`,
  `TrapEvent`, status codes, version negotiation, layout table).
- Generated C header `kernel/os-liu/include/liu/host.h`.
- `liu-elf`: `no_std` ELF64 parser and kernel image load planner.
- `liu-platform`: host probing (Renux detected at runtime), anonymous shared
  memory, process creation with process descriptors, Capsicum sandbox, native
  FreeBSD ptrace wrappers and the Renux `PT_SET_SC_RET` primitive.
- `xtask`: `gen-header`, `check-header`, `check-abi`, `style`, `ci`.
- `liu-platform`: fixed-address memory (`Region`, `SharedMemory::map_at`)
  that never replaces an existing mapping, and `page_size`.
- `liu-runtime` and the `liu-kernel` binary (M1, first slice): loads a
  freestanding kernel image at its link address with per-segment
  permissions, maps guest physical memory after it, builds `BootInfo` and
  enters `liu_kernel_entry` on a vCPU thread. Serves `log_write` and `panic`.
- `tests/abi-kernel`: freestanding `no_std` test kernel linked at
  `0x60000000` like arch/um, run by `cargo xtask test-abi` in CI.
