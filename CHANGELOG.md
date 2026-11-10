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
