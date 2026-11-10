# Architecture Decision Records

Each record captures one decision, its context, and its consequences. Records
are immutable once accepted; a later record may supersede an earlier one.

| ADR | Title | Status |
|---|---|---|
| [0001](0001-real-linux-kernel-in-userspace.md) | Run the real Linux kernel in user space | Accepted |
| [0002](0002-freestanding-kernel-rust-runtime.md) | Freestanding kernel image hosted by a Rust runtime | Accepted |
| [0003](0003-devices-via-vhost-user.md) | Devices as vhost-user backends | Accepted |
| [0004](0004-pluggable-trap-backends.md) | Pluggable trap backends | Accepted |
| [0005](0005-no-linux-code-in-renux.md) | No Linux code in the Renux kernel | Accepted |
| [0006](0006-portable-platform-layer.md) | Portable platform layer | Accepted |
| [0007](0007-liu-as-a-renux-component.md) | LIU as a Renux component, delivered as a package | Proposed |

New records use the next number and the template below.

```markdown
# ADR NNNN: Title

## Status
Proposed | Accepted | Superseded by NNNN

## Context
## Decision
## Consequences
## Alternatives considered
```
