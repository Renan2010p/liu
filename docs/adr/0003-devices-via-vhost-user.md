# ADR 0003: Devices as vhost-user backends

## Status

Accepted

## Context

UML's traditional drivers (`ubd`, `hostfs`, `net_user`) perform host I/O
directly from the kernel process through libc, which conflicts with ADR 0002
and with confining the kernel process. Upstream already ships `virtio_uml`,
a virtio transport over the vhost-user protocol.

## Decision

Disable the libc-based UML drivers. Provide every device as a vhost-user backend
in its own Rust process (`liu-dev-console`, `-blk`, `-net`, `-fs`, `-rng`),
sharing guest physical memory by descriptor. The kernel obtains device sockets
only through `host_ops.vhost_connect(name)`, which the supervisor controls.

## Consequences

- Standard Linux virtio drivers are used unchanged.
- Each backend holds exactly one host resource and runs in capability mode.
- A crash in a backend affects one device, not the kernel process.
- Requires a vhost-user implementation that works on Renux (no `epoll`).

## Alternatives considered

- **Port UML drivers to the host ABI.** Rejected: more kernel patches, weaker isolation.
- **Custom LIU paravirtual devices.** Rejected: duplicates virtio.
