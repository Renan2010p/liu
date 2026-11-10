# ADR 0006: Portable platform layer

## Status

Accepted

## Context

LIU was first designed for Renux only. Nothing in the core design requires
Renux: the kernel image is freestanding (ADR 0002), the host ABI is
Linux-agnostic and host-agnostic, and devices speak vhost-user (ADR 0003).
Only a few things differ between Unix hosts: how a process's system calls are
intercepted, how shared memory and event loops are created, and how a process
is sandboxed. Making LIU available on other systems widens its user base and
its test coverage, and gives every Unix a way to run real Linux software
without a hypervisor.

## Decision

1. All host-specific code lives in one crate, `liu-platform`, behind traits
   (`Platform`, `SharedMemory`, `GuestSpace`, `TrapBackend`, `EventLoop`,
   `Sandbox`). `#[cfg(target_os = ...)]` is forbidden elsewhere.
2. `Platform::probe()` detects the running host's capabilities at startup and
   picks the fastest trap backend available.
3. Renux is the reference platform. It is handled as FreeBSD plus
   runtime-detected extensions (`PT_SET_SC_RET`, later `trapvec`), so one
   FreeBSD build serves both.
4. Hosts are grouped in support tiers (see `architecture.md` §7.4). Tier 1:
   Renux, FreeBSD, Linux. Tier 2: NetBSD, DragonFly, macOS (Intel). Tier 3:
   OpenBSD, illumos, arm64 hosts.
5. Unprivileged, portable defaults are preferred: user-mode networking instead
   of TAP devices; the best available sandbox per host.

## Consequences

- One kernel image and one set of device backends for every host.
- Each trap backend needs its own conformance tests and CI coverage.
- Security strength varies by host; the documentation must say so.
- arm64 hosts remain out of reach until `arch/um` is ported to arm64.
- OpenBSD is blocked on the absence of system call stops in its `ptrace(2)`.

## Alternatives considered

- **Renux only.** Rejected: limits adoption for no technical benefit.
- **A CPU emulator (TCG) for hosts without syscall interception.** Kept as a
  possible Tier 3 fallback; too slow to be a primary backend.
- **Hardware virtualization everywhere.** Rejected as the default: needs
  privileges or entitlements on many hosts; kept as the `hvf`/`kvm` backends.
