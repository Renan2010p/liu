# LIU — Linux In Userspace

**Run a real Linux kernel as an ordinary process on [Renux](https://renuxproject.github.io) and other Unix systems.**

LIU loads an unmodified upstream Linux kernel into user space and lets Linux
applications run on it. The host provides only the "hardware" — memory, CPU
time, timers, interrupts and devices — through a small, versioned host ABI
written in Rust. Unlike the in-kernel linuxulator, LIU translates no system
calls and needs no Linux code in the host kernel.

| Tier | Hosts (x86_64) |
|---|---|
| 1 | Renux (reference platform), FreeBSD, Linux |
| 2 | NetBSD, DragonFly, macOS (Intel) |
| 3 | OpenBSD, illumos, arm64 hosts (research) |

> **Status:** design phase. See [docs/architecture.md](docs/architecture.md).
> A Phase 0 prototype validated the execution model and the required Renux
> ptrace primitive (`PT_SET_SC_RET`).

## Why

| | linuxulator | LIU |
|---|---|---|
| Who implements Linux syscalls | host kernel (translation) | the real Linux kernel |
| `io_uring`, eBPF, namespaces, cgroups | missing or partial | available |
| Linux code in the host kernel | yes | none |
| Isolation | in-kernel | unprivileged, sandboxed process |
| Cost | low per syscall | a trap per syscall; native speed otherwise |

## Architecture at a glance

```
Linux applications (unmodified)
        │ Linux syscalls (trapped)
Linux kernel (upstream arch/um, freestanding image)      C, GPL-2.0
        │ LIU host ABI (#[repr(C)] function tables)
liu-runtime + vhost-user device backends                 Rust, MPL-2.0
liu-platform (the only host-specific code)               Rust, MPL-2.0
        │ native host syscalls
Host kernel: Renux, FreeBSD, Linux, NetBSD, macOS, ...
```

## Documentation

- [Architecture](docs/architecture.md)
- [Architecture decision records](docs/adr/)
- [Rust style (KNF)](docs/style.md)

## License

LIU is licensed under the [Mozilla Public License 2.0](LICENSE).

The Linux kernel and the patches under `kernel/` are licensed under GPL-2.0-only.
See [docs/architecture.md §17](docs/architecture.md#17-licensing).
