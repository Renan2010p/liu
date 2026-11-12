# Changelog

All notable changes to LIU are documented here.

## Unreleased

### Added

- Architecture document, ADRs 0001–0006 and 0008, KNF style guide.
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
- `liu-platform`: host clocks (`clock_monotonic_ns`, `clock_realtime_ns`)
  and entropy (`fill_random`: `getrandom` or `getentropy`).
- `liu-runtime`: the time, random and cpu groups of `host_ops` (`clock_*`,
  `get_random`, `timer_set`, `timer_cancel`, `irq_*`, `idle`), with
  interrupts delivered at safe points (ADR 0008).
- Host ABI 0.2: `BootInfo` gains `kmap_base` and `kmap_size`; `kmap`,
  `kprotect` and `kstack_*` semantics are documented in `host_ops.rs`.
- `liu-platform`: `Window` (inaccessible reserved range where shared memory
  is mapped page by page) and `Stack` (stack with a guard page).
- `liu-physmem`: guest physical memory with its linear map and checked
  physical ranges.
- `liu-runtime`: kernel memory (`kmap`, `kunmap`, `kprotect`,
  `kstack_alloc`, `kstack_free`) confined to a 1 GiB window after physmem.
  This completes M1.
- `liu-platform`: `TrapBackend` and `GuestSpace` traits and
  `open_trap_backend`; the `renux-setscret` backend (M2): one ptrace stop
  per guest system call, results delivered with `PT_SETREGS` +
  `PT_SET_SC_RET(EJUSTRETURN)` so the guest sees exact Linux semantics;
  faults with derived access type; FS/GS base. Tests run hand-assembled
  Linux code on Renux (6.6 µs per system call).
- `liu-platform`: the guest stub. Guest spaces now contain only the stub
  and physmem mapped by `GuestSpace::map`; address-space changes
  (`map`, `unmap`, `protect`) are validated, queued and applied in batches
  of up to 63 by the stub.
- `tests/abi-kernel`: freestanding `no_std` test kernel linked at
  `0x60000000` like arch/um, run by `cargo xtask test-abi` in CI.
