# ADR 0005: No Linux code in the Renux kernel

## Status

Accepted

## Context

LIU may need kernel support for performance or correctness. Adding a
Linux-specific path would recreate the linuxulator problem.

## Decision

Any Renux kernel change made for LIU must be a generic primitive that:

1. contains no Linux syscall numbers, structures or semantics;
2. is useful to other consumers (debuggers, sandboxes, emulators);
3. is tested on its own, independently of LIU.

`PT_SET_SC_RET` is the reference example.

## Consequences

- Renux stays independent of Linux.
- Some optimizations require more general designs.
- Kernel patches are maintained in the Renux tree, not in LIU.
