# ADR 0004: Pluggable trap backends

## Status

Accepted

## Context

Every guest system call and page fault must be intercepted and delivered to the
Linux kernel without being executed by Renux. This is the dominant performance
cost and the component most dependent on Renux kernel features. The Phase 0
work added `PT_SET_SC_RET` to Renux and verified a SYSEMU-equivalent sequence.

## Decision

Define a `TrapBackend` trait in `liu-platform`. The first backend uses native
Renux ptrace (`PT_TO_SCE`, `PT_SET_SC_RET`, `PT_GETREGS`/`PT_SETREGS`). Other
hosts get their own backends (ADR 0006). A faster backend based on a generic
Renux syscall-redirection primitive (`trapvec`) may be added later without
changing the host ABI or the kernel port.

## Consequences

- The kernel port depends on `as_run` returning a `TrapEvent`, not on ptrace.
- Backends can be benchmarked side by side.
- The ptrace backend works on the current Renux kernel.

## Alternatives considered

- **Hard-wire ptrace as UML does.** Rejected: blocks future optimization.
- **Signal-based in-process trapping.** Rejected: guest and kernel would share an
  address space, losing isolation between Linux processes.
