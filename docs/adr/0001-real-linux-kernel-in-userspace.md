# ADR 0001: Run the real Linux kernel in user space

## Status

Accepted

## Context

Renux, like FreeBSD, can run Linux binaries through the in-kernel linuxulator,
which translates each Linux system call into a native one. That approach is
permanently incomplete: `io_uring`, `bpf`, namespaces, cgroups, `pidfd_*`,
`userfaultfd`, `fanotify` and many `ioctl`s are missing or stubbed, and it puts
a large Linux-specific surface inside the host kernel.

## Decision

Run an unmodified upstream Linux kernel as an unprivileged Renux process, based
on the upstream `arch/um` (User-Mode Linux) architecture. Linux applications run
on that kernel. Renux supplies memory, CPU time, timers, interrupts and devices.

## Consequences

- Linux semantics come from Linux itself; compatibility tracks upstream.
- The Renux kernel needs no Linux ABI code.
- Syscall-heavy workloads pay a trap cost per syscall (see ADR 0004).
- LIU must maintain a port of the `arch/um` host layer.

## Alternatives considered

- **Extend the linuxulator.** Rejected: unbounded maintenance, in-kernel attack surface.
- **gVisor-style user-space reimplementation.** Rejected: reimplements Linux.
- **bhyve.** Requires hardware virtualization and root; remains a separate option.
- **LKL (Linux Kernel Library).** Runs Linux as a library but has no model for
  running separate user processes; `arch/um` already does.
