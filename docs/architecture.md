# LIU Architecture

| | |
|---|---|
| **Status** | Draft, revision 1 |
| **Scope** | LIU host runtime, host ABI, kernel port, devices, build, roadmap |
| **Supersedes** | The Phase 0 prototype (`liu` repository, commit `73410a5`) |
| **License** | MPL-2.0 (LIU code); GPL-2.0 (Linux kernel and its patches) |

---

## 1. Summary

LIU (**Linux In Userspace**) runs an unmodified, real Linux kernel as an ordinary
unprivileged process on Renux and on other Unix systems: FreeBSD, Linux,
NetBSD and macOS among them. Linux applications run *on that kernel*, not on the
host: their system calls are serviced by real Linux code. The host provides only
the "hardware" — memory, CPU time, timers, interrupts, and devices — through a
small, versioned, Linux-agnostic **host ABI** implemented in Rust. Renux is the
reference platform and gets dedicated fast paths; every other host is reached
through a portable platform layer (§7).

This is the opposite of the FreeBSD linuxulator. The linuxulator translates each
Linux system call into a native one inside the host kernel, which leaves many
interfaces missing or stubbed (`io_uring`, `bpf`, namespaces, cgroups,
`pidfd_*`, `userfaultfd`, `fanotify`, ...). LIU translates nothing at the
syscall level. It virtualizes the machine underneath Linux, so every Linux
interface the kernel supports is available, and the Renux kernel carries no
Linux code.

```
        Linux applications (unmodified ELF, glibc or musl)
                         │  Linux syscalls (trapped, never executed by the host)
        ┌────────────────▼─────────────────────────────────┐
        │  Linux kernel (upstream arch/um, freestanding)   │  GPL-2.0, C
        │  VFS · scheduler · net stack · io_uring · eBPF   │
        ├──────────────────────────────────────────────────┤
        │  os-liu: thin C shim over the host ABI           │  GPL-2.0, C
        └────────────────┬─────────────────────────────────┘
                         │  LIU host ABI (#[repr(C)] function tables)
        ┌────────────────▼─────────────────────────────────┐
        │  liu-runtime: memory · guest processes · traps · │  MPL-2.0, Rust
        │  timers · interrupts · event loop                │
        ├──────────────────────────────────────────────────┤
        │  Device backends (vhost-user, separate processes)│  MPL-2.0, Rust
        │  console · block · net · fs                      │
        └────────────────┬─────────────────────────────────┘
        ├──────────────────────────────────────────────────┤
        │  liu-platform: the only host-specific code       │  MPL-2.0, Rust
        └────────────────┬─────────────────────────────────┘
                         │  native host syscalls (ptrace, shm, kqueue/epoll, sandbox)
        ┌────────────────▼─────────────────────────────────┐
        │  Host kernel: Renux · FreeBSD · Linux · NetBSD · │
        │  macOS ... (no Linux compatibility code needed)  │
        └──────────────────────────────────────────────────┘
```

## 2. Goals and non-goals

### Goals

1. Run unmodified Linux binaries against a **real** Linux kernel on Renux and other Unix hosts.
2. Keep the Renux kernel free of Linux-specific code. Any kernel support LIU
   needs must be a generic, independently useful primitive.
3. Write every component LIU authors in **Rust**, and keep the unavoidable C
   (code that lives inside the Linux tree) as small and mechanical as possible.
4. Run unprivileged and confined: no root, no kernel module, Capsicum-sandboxed.
5. Track an upstream Linux LTS release with a small, rebasable patch series.
6. **Portability.** Run on any Unix host that offers a way to intercept a
   process's system calls. Host-specific code is confined to the platform layer;
   the kernel image, the host ABI and the device backends are identical on
   every host.

### Non-goals

- Reimplementing the Linux syscall surface (the gVisor approach).
- Translating Linux syscalls into host syscalls (the linuxulator approach).
- Hardware virtualization. bhyve already covers that; LIU must work without
  VT-x and inside jails.
- Graphics acceleration in the first releases.

## 3. Lessons from the Phase 0 prototype

The prototype proved the concept and identified exactly where it broke. This
design is shaped by those results.

| Finding | Consequence for this design |
|---|---|
| A real UML 6.12 kernel boots on a real Linux host, and a Rust guest benchmark runs inside it. | The execution model (Linux kernel as a process) is sound. Keep `arch/um`. |
| Booting under the linuxulator fails: `PTRACE_SYSEMU` is missing and `PTRACE_POKEUSER` is broken there. | Never depend on a host's Linux emulation. Use native host primitives only. |
| `PT_SET_SC_RET` was added to the Renux kernel as a generic ptrace request and verified (`ptrace-setscret-test.c`, `sysemu-test.c`). | This is the trap primitive for the first backend. It is generic and stays. |
| `arch/x86/um/user-offsets.c` and `os-Linux` collide with FreeBSD libc headers; a dozen shims (`epoll`, `sys/vfs.h`, `pty.h`, ...) were needed. | Stop compiling Linux host code against the host libc at all. |
| The natively built UML binary aborts before `main`: the UML linker script is incompatible with the FreeBSD C runtime. | Stop linking the kernel as a host libc program. Load it ourselves. |
| The host-side C port (`os-Renux`) grew to many files of libc emulation. | Move host logic into Rust behind a narrow ABI; the C side only forwards. |

The central change is therefore: **the Linux kernel becomes a freestanding image
loaded and hosted by a Rust runtime**, instead of a FreeBSD executable linked
against libc.

## 4. Key design decisions

Each decision has an ADR in [`adr/`](adr/).

| # | Decision | ADR |
|---|---|---|
| D1 | Run the real Linux kernel in user space, based on upstream `arch/um`. | [0001](adr/0001-real-linux-kernel-in-userspace.md) |
| D2 | Build the kernel freestanding; a Rust runtime loads it and provides host services through `#[repr(C)]` function tables. | [0002](adr/0002-freestanding-kernel-rust-runtime.md) |
| D3 | Devices are vhost-user backends in separate Rust processes, driven by the upstream `virtio_uml` driver. | [0003](adr/0003-devices-via-vhost-user.md) |
| D4 | Guest syscall trapping sits behind a `TrapBackend` trait. Backend 1 is native ptrace with `PT_SET_SC_RET`. | [0004](adr/0004-pluggable-trap-backends.md) |
| D5 | Renux kernel changes must be generic primitives with no Linux knowledge. | [0005](adr/0005-no-linux-code-in-renux.md) |
| D6 | LIU is portable across Unix hosts through a platform layer; Renux is the reference platform with runtime-detected fast paths. | [0006](adr/0006-portable-platform-layer.md) |

## 5. Execution model

### 5.1 Processes

A running LIU instance consists of the following host processes:

```
liu (supervisor, Rust)                          ── pdfork, Capsicum
 ├── liu-kernel  (runtime + Linux kernel image) ── one process, N host threads
 │    ├── guest mm #1   (Linux process A)       ── traced by liu-kernel
 │    ├── guest mm #2   (Linux process B)
 │    └── ...
 ├── liu-dev-console   (vhost-user backend)
 ├── liu-dev-blk       (vhost-user backend)
 ├── liu-dev-net       (vhost-user backend)
 └── liu-dev-fs        (vhost-user backend)
```

- **Supervisor** (`liu`): parses configuration, creates the shared memory
  objects and device sockets, spawns everything with `pdfork(2)`, holds the
  process descriptors, and owns lifecycle and logging. It never runs guest code.
- **Kernel process** (`liu-kernel`): the Rust runtime plus the Linux kernel image
  mapped into the same address space. Linux kernel code runs here.
- **Guest address-space processes**: one host process per Linux `mm_struct`
  (or one vCPU address space with the `hvf`/`kvm` backends). Linux user code
  runs here. Every syscall it makes is trapped and handed to the Linux kernel;
  the host never executes it.
- **Device backends**: one process per device class, each with only the
  capabilities it needs (a file descriptor for a disk image, a `tap(4)`
  descriptor, a directory descriptor).

### 5.2 Guest physical memory

Guest "physical" RAM is a single anonymous shared memory object, created by
the platform layer (`shm_open(SHM_ANON)` on Renux/FreeBSD, `memfd_create` on
Linux; see §7.5). It is mapped:

- into the kernel process as the kernel's linear map;
- into each guest process, page by page, as directed by the Linux kernel's page
  tables (UML's model: user mappings are `mmap` of offsets in the physmem
  object);
- into device backends, which receive the descriptor over the vhost-user socket
  and perform DMA-style access directly.

No page is copied across the boundary. A page fault in a guest process becomes a
`SIGSEGV` stop, the trap backend reports it to the kernel, Linux resolves it in
its own page tables, and the runtime applies the resulting mapping in the guest
process.

### 5.3 Guest address-space operations

Mapping changes inside a guest process are performed by a small **stub** mapped
at a fixed address in every guest process (as in upstream UML). The runtime
writes a batch of `mmap`/`munmap`/`mprotect` requests into the stub's shared data
page and resumes the stub, which executes them natively and traps back. Batching
keeps the number of context switches per `execve`/`mmap` low.

The stub is a few hundred bytes of position-independent code generated from Rust
(`#![no_std]`, `#[naked]` functions) and embedded in the runtime.

### 5.4 System call path

```
guest process            trap backend          liu-runtime            Linux kernel
     │ syscall                 │                     │                      │
     ├────────────────────────►│ stop (SCE)          │                      │
     │                         ├────────────────────►│ read regs            │
     │                         │                     ├─────────────────────►│ kernel_ops.trap(SYSCALL, regs)
     │                         │                     │                      │ (real Linux syscall code)
     │                         │                     │◄─────────────────────┤ return regs / mm ops
     │                         │ skip + set retval   │ flush stub batch     │
     │◄────────────────────────┤ (PT_SET_SC_RET)     │                      │
     │ continues               │                     │                      │
```

### 5.5 Kernel threads, interrupts and time

- The Linux kernel runs uniprocessor (`CONFIG_SMP=n`) on one **vCPU host
  thread**. SMP is a later milestone.
- Host events (timers, device notifications, console input) are collected by an
  **event thread** using the platform event loop (`kqueue` or `epoll`).
- The event thread posts interrupt numbers into a lock-free pending bitmap and
  sends a signal to the vCPU thread. The runtime's signal handler calls
  `kernel_ops.irq()`, which enters Linux's interrupt path exactly as UML does
  for `SIGIO`/`SIGALRM` today.
- The Linux kernel never calls `sigaction`, `timer_create`, `kqueue` or `epoll` itself.
  It asks the runtime through the host ABI.

## 6. Host ABI

The host ABI is the only interface between Linux and the host. It is defined once,
in Rust, in `liu-abi`, and a C header (`include/liu/host.h`) is generated from it
with `cbindgen`. Both sides are checked against the same layout in CI.

### 6.1 Shape

Two function tables, exchanged at entry:

```c
/* Generated from liu-abi. SPDX-License-Identifier: MPL-2.0 OR GPL-2.0-only */
struct liu_host_ops;    /* kernel -> runtime: services the host provides      */
struct liu_kernel_ops;  /* runtime -> kernel: entry points Linux registers    */

/* Entry point of the freestanding Linux image. */
int liu_kernel_entry(const struct liu_host_ops *host,
                     struct liu_kernel_ops *kernel,
                     const struct liu_boot_info *boot);
```

Every table begins with a header:

```rust
#[repr(C)]
pub struct TableHeader {
    pub magic: u32,        // b"LIU\0"
    pub abi_major: u16,    // incompatible changes
    pub abi_minor: u16,    // additive changes
    pub size: u32,         // size_of::<Table>(), for forward compatibility
    pub _reserved: u32,
}
```

The runtime refuses to start a kernel whose `abi_major` differs. A newer minor
version is accepted; entries beyond `size` are treated as absent.

### 6.2 `liu_host_ops` groups

The authoritative definition is `crates/liu-abi/src/host_ops.rs`.

| Group | Operations | Notes |
|---|---|---|
| **console** | `log_write`, `panic` | Early boot output before devices exist. `panic` does not return. |
| **kernel memory** | `kmap`, `kunmap`, `kprotect` | Map physmem ranges at kernel-chosen addresses. Physmem itself is mapped at `boot.physmem_base` before entry. |
| **address spaces** | `as_create`, `as_destroy`, `as_map`, `as_unmap`, `as_protect`, `as_flush` | Guest address spaces; changes are batched until `as_flush` or `as_run`. |
| **execution** | `as_run(space, regs, event)`, `as_get_fpu`, `as_set_fpu` | Run guest user code until the next trap. TLS bases are part of `GuestRegs`. |
| **cpu** | `irq_enable`, `irq_disable`, `irq_save`, `irq_restore`, `idle` | `idle` blocks the vCPU until an interrupt is pending. |
| **time** | `clock_monotonic`, `clock_realtime`, `timer_set`, `timer_cancel` | Nanoseconds; tickless one-shot deadline. |
| **stacks** | `kstack_alloc`, `kstack_free` | Stacks for Linux kernel threads, switched inside the vCPU thread. |
| **random** | `get_random` | Host entropy. |
| **devices** | `dev_connect`, `chan_send`, `chan_recv`, `event_create`, `event_signal`, `irq_bind`, `handle_close` | `dev_connect` takes a device *name* from the supervisor configuration, never a path. Channels carry vhost-user messages; handles travel out of band. |

Rules:

- Status codes are a closed `#[repr(i32)]` enum defined in `liu-abi`. Host
  `errno` values never cross the boundary.
- Pointers passed to the host ABI are kernel virtual addresses in the kernel
  process. Guest user memory is accessed only through physmem.
- No operation names a file path, a socket address or a Linux structure.

### 6.3 `liu_kernel_ops`

| Entry | Called when |
|---|---|
| `irq(pending_mask)` | the event thread raised one or more interrupts |
| `timer()` | the one-shot deadline expired |
| `kernel_fault(addr, access, ip)` | a fault occurred in kernel code (e.g. vmalloc area) |
| `shutdown(reason)` | the supervisor requests an orderly power-off |

Guest traps (syscalls, faults, signals) are not callbacks: they are the return
value of `as_run`, which keeps the control flow identical to UML's `userspace()`
loop.

### 6.4 Trap events

```rust
#[repr(C)]
pub struct TrapEvent
{
    pub kind: u32,      // TRAP_SYSCALL, TRAP_PAGE_FAULT, TRAP_SIGNAL, TRAP_EXITED
    pub signo: u32,     // TRAP_SIG_* (LIU's own numbering) for TRAP_SIGNAL
    pub access: u32,    // ACCESS_READ | ACCESS_WRITE | ACCESS_EXEC for faults
    pub reserved: u32,
    pub addr: u64,      // faulting address
    pub code: u64,      // syscall number, or exit status
}
```

Registers are not part of the event: `as_run` updates the caller's
`GuestRegs` in place. Kinds are plain integers rather than a Rust enum, so an
unexpected value from the other side is an error, not undefined behavior.

## 7. Platform layer and trap backends

### 7.1 Portability model

Everything above the platform layer is host-independent:

- The kernel image is a freestanding ELF that links against no host library
  and uses the System V AMD64 calling convention, which every x86_64 Unix
  shares. The same image runs on every host.
- The host ABI, the device protocol (vhost-user) and the device backends are
  identical on every host.

Everything host-specific lives in `liu-platform`, which defines traits and
provides one implementation per host. No other crate may use
`#[cfg(target_os = ...)]`.

```rust
pub trait Platform
{
    type Memory: SharedMemory;   // physmem object: SHM_ANON, memfd, shm_open
    type Space: GuestSpace;      // one guest address space
    type Trap: TrapBackend<Space = Self::Space>;
    type Events: EventLoop;      // kqueue, epoll
    type Sandbox: Sandbox;       // Capsicum, Landlock+seccomp, sandbox_init

    fn
    probe() -> Result<Capabilities, Error>;
}

pub trait TrapBackend
{
    type Space;

    fn
    create_space(&mut self, physmem: &dyn SharedMemory) -> Result<Self::Space, Error>;

    fn
    run(&mut self, space: &mut Self::Space, regs: &mut GuestRegs) -> Result<TrapEvent, Error>;
}
```

`Platform::probe()` runs at startup and selects the fastest backend the running
host supports. A Renux kernel is detected as FreeBSD plus extensions, so a
stock FreeBSD build of LIU automatically uses the Renux fast path when it runs
on Renux.

### 7.2 Host requirements

A host can run LIU when it is **x86_64** and offers:

1. a way to stop a process at system call entry, read and write its registers,
   and **prevent the host from executing the call**;
2. shared memory that can be mapped into several processes;
3. a readiness-based event mechanism.

Requirements 2 and 3 are met by every modern Unix. Requirement 1 decides the
trap backend.

### 7.3 Trap backends

| Backend | Hosts | Mechanism | Stops per syscall | Status |
|---|---|---|---|---|
| `renux-setscret` | Renux | `PT_TO_SCE`, then `PT_SET_SC_RET` skips the call and sets the result | 1 | Kernel primitive implemented and verified |
| `freebsd-rewrite` | FreeBSD, DragonFly | `PT_TO_SCE`, rewrite the call to a harmless one, then `PT_TO_SCX` and set the result with `PT_SETREGS` | 2 | To validate on stock FreeBSD |
| `linux-sysemu` | Linux | `PTRACE_SYSEMU` (what upstream UML uses) | 1 | Proven upstream |
| `netbsd-syscallemu` | NetBSD | `PT_SYSCALL` + `PT_SYSCALLEMU` | 1 | To validate |
| `hvf` | macOS (Intel) | Hypervisor.framework: guest processes run in ring 3 of a vCPU whose page tables map physmem; `syscall` exits to the runtime. No VM, no guest firmware: only guest user code runs in the vCPU | 0 (VM exit) | Design |
| `kvm` | Linux | Same as `hvf`, through `/dev/kvm` (the gVisor KVM platform model) | 0 (VM exit) | Design |
| `trapvec` | Renux | Generic Renux syscall-redirection primitive: a marked process's syscalls are written to a shared-memory mailbox | 0 (mailbox) | Design (M6) |

The `trapvec` primitive must remain Linux-agnostic: it delivers raw register
state and never interprets syscall numbers. It is useful for any user-space
emulator or sandbox, which is what justifies adding it to Renux (D5).

### 7.4 Support tiers

| Tier | Meaning | Hosts |
|---|---|---|
| **1** | Built and tested in CI on every change; release blocker | Renux, FreeBSD, Linux (x86_64) |
| **2** | Built in CI; tested before releases | NetBSD, DragonFly, macOS on Intel |
| **3** | Research; no guarantees | OpenBSD, illumos, any arm64 host |

Known limits:

- **arm64 hosts** (Apple Silicon, Raspberry Pi, Ampere) need an arm64 port of
  `arch/um`, which does not exist upstream. The host ABI is designed so that
  only `GuestRegs` and the trap backends are architecture-specific.
- **OpenBSD** `ptrace(2)` has no system call stops; a backend would need a new
  mechanism.
- **macOS** `ptrace(2)` cannot read registers; the `hvf` backend is the only
  route and requires the `com.apple.security.hypervisor` entitlement
  (ad-hoc signing is enough).

### 7.5 Other host differences

| Concern | Renux / FreeBSD | Linux | NetBSD | macOS |
|---|---|---|---|---|
| Physmem object | `shm_open(SHM_ANON)` | `memfd_create` | `memfd_create` / `shm_open` | `shm_open` + `shm_unlink` |
| Event loop | `kqueue` | `epoll` | `kqueue` | `kqueue` |
| Device kick | `eventfd` | `eventfd` | `eventfd` | `pipe` pair |
| Process handle | `pdfork` | `pidfd` | `fork` + pid | `fork` + pid |
| Sandbox | Capsicum, jail | Landlock + seccomp | `chroot` (best effort) | `sandbox_init` (best effort) |
| Network (default) | user-mode NAT | user-mode NAT | user-mode NAT | user-mode NAT |
| Network (optional) | `tap(4)` | TUN/TAP | `tap(4)` | `vmnet` |

## 8. Devices

LIU reuses the upstream `virtio_uml` driver (`arch/um/drivers/virtio_uml.c`),
which implements virtio transports over the **vhost-user** protocol. Devices are
therefore ordinary vhost-user backends, each a separate Rust process:

| Crate | Linux driver | Backend |
|---|---|---|
| `liu-dev-console` | `virtio_console` | stdio or `pts(4)` |
| `liu-dev-blk` | `virtio_blk` | raw image file, ZFS zvol, or `md(4)` via a descriptor |
| `liu-dev-net` | `virtio_net` | user-mode NAT stack (default, unprivileged, portable); host TAP device (optional) |
| `liu-dev-fs` | `virtio_fs` / `9p` | a Capsicum directory descriptor (`openat` beneath it only) |
| `liu-dev-rng` | `virtio_rng` | `getrandom(2)` |

Shared building blocks live in `liu-vhost` (protocol, virtqueue access over
physmem). Event loops and kick descriptors come from `liu-platform`, so the
backends contain no host-specific code.

The device boundary is the security boundary for I/O: a compromised Linux kernel
can only send vhost-user messages and virtqueue descriptors to backends that
validate every descriptor against the physmem bounds.

## 9. Kernel side (C, GPL-2.0)

The C code LIU maintains is limited to a patch series against a pinned Linux LTS
tree:

```
kernel/
  VERSION                     pinned upstream version (6.12.y LTS)
  patches/
    0001-um-add-os-liu-host-layer.patch
    0002-um-freestanding-build-mode.patch
    0003-um-x86-user-offsets-without-host-headers.patch
    ...
  configs/
    liu_defconfig             UP, virtio_uml, no hostfs, no host libc drivers
  os-liu/                     source of the host layer (copied into arch/um/)
    host.c                    os_* functions forwarding to liu_host_ops
    entry.c                   liu_kernel_entry, table registration
    include/liu/host.h        generated, never edited
```

Principles:

- `os-liu` implements the existing `os_*` contract of `arch/um` by forwarding to
  `liu_host_ops`. It contains no policy and no libc calls.
- The kernel is built with `-ffreestanding -nostdlib`, linked as a static
  position-independent ELF with a LIU-specific linker script, and exports only
  `liu_kernel_entry`. This removes the host libc headers and the host C runtime
  from the build entirely, which eliminates the two Phase 0 blockers.
- UML drivers that require host libc (`hostfs`, `ubd`, `net_user`, `mconsole`
  helpers) are disabled; their roles are filled by vhost-user devices.
- Patches are kept small and rebased on each LTS point release by CI.

## 10. Rust workspace

```
liu-project/
├── Cargo.toml                 workspace
├── rust-toolchain.toml
├── LICENSE                    MPL-2.0
├── crates/
│   ├── liu-abi/               host ABI types, tables, status codes (no_std)
│   ├── liu-elf/               ELF64 loader for the kernel image (no_std)
│   ├── liu-platform/          platform traits + one module per host (the only cfg(target_os) code)
│   │   └── src/{renux,freebsd,linux,netbsd,macos}/
│   ├── liu-physmem/           bounds-checked guest physical memory access
│   ├── liu-trap/              guest stub and host-independent trap handling
│   ├── liu-runtime/           kernel process: loader, host_ops impl, vCPU, event thread
│   ├── liu-vhost/             vhost-user protocol, virtqueues
│   ├── liu-dev-console/       device backend binary
│   ├── liu-dev-blk/           device backend binary
│   ├── liu-dev-net/           device backend binary
│   ├── liu-dev-fs/            device backend binary
│   ├── liu-config/            instance configuration (TOML) and validation
│   ├── liu-rootfs/            rootfs image tooling
│   └── liu/                   supervisor and CLI
├── kernel/                    Linux patch series and os-liu (GPL-2.0)
├── xtask/                     build orchestration (cargo xtask ...)
├── tests/                     integration and boot tests
└── docs/
```

Crate rules:

- `unsafe` is allowed only in `liu-platform`, `liu-physmem`, `liu-trap` and the
  `host_ops` glue in `liu-runtime`. Every other crate is `#![forbid(unsafe_code)]`.
- `#[cfg(target_os = ...)]` is allowed only in `liu-platform`.
- `liu-abi` and `liu-elf` are `no_std` so they can also be used by the stub and
  by test kernels.
- All Rust code follows the KNF style in [`style.md`](style.md).

### Dependency graph

```
liu (CLI) ──► liu-config, liu-platform
liu-runtime ──► liu-abi, liu-elf, liu-physmem, liu-trap, liu-platform
liu-trap ──► liu-abi, liu-physmem, liu-platform
liu-dev-* ──► liu-vhost ──► liu-physmem, liu-platform
```

## 11. Command line and configuration

```sh
liu run --kernel build/linux.liu --rootfs fedora.img -- /bin/bash
liu run --config instance.toml
liu ps | liu stop <name>
```

```toml
[instance]
name = "fedora"
memory = "2G"

[kernel]
image = "build/linux.liu"
cmdline = "root=/dev/vda rw console=hvc0"

[[device]]
kind = "blk"
path = "/var/liu/fedora.img"

[[device]]
kind = "net"
tap = "tap0"

[[device]]
kind = "fs"
tag = "home"
path = "/home/renan"
read_only = false
```

## 12. Security model

| Layer | Mechanism |
|---|---|
| Supervisor | Opens every host resource up front, then passes descriptors; enters the platform sandbox before starting the kernel. |
| Kernel process | Platform sandbox (Capsicum capability mode on Renux/FreeBSD, Landlock + seccomp on Linux). Holds only: physmem, vhost sockets, its handles for guest spaces. Cannot open paths or sockets. |
| Guest processes | Created by the kernel process; every syscall is trapped. Even with a trap-backend bug they run in capability mode with no descriptors. |
| Device backends | Each in capability mode with exactly one resource (an image fd, a tap fd, a directory fd). Descriptor validation against physmem bounds. |
| Instance | Optionally inside a jail (Renux/FreeBSD) or namespaces (Linux), with resource limits. |

A Linux kernel compromise yields control of an unprivileged, sandboxed host
process. It cannot reach the host kernel through any Linux-specific code path,
because none exists. The strength of the sandbox depends on the host: it is
strongest on Renux and FreeBSD (Capsicum) and Linux (Landlock + seccomp), and
best effort elsewhere.

## 13. Performance model

| Path | Crosses to the host? | Cost driver |
|---|---|---|
| CPU-bound guest code | no | native speed |
| Syscall served from Linux caches (`getpid`, cached `read`, pipes, futex) | trap only | trap backend round-trip |
| Page fault / `mmap` | trap + stub batch | number of mapping changes |
| Block / network I/O | virtqueue kick | device rate, batched |
| Timer tick | none (tickless) | one-shot deadlines |

Every milestone records: syscall round-trip latency, page-fault latency,
`fork`+`exec` time, boot-to-shell time, and memory overhead. The Phase 0
TCG-emulated reference (≈404 µs per `getpid`) is not representative; the first
bare-metal numbers on each Tier 1 host are a milestone deliverable.

## 14. Renux kernel primitives

LIU needs no Renux-specific kernel support to run (stock FreeBSD is a supported
host). The primitives below are Renux fast paths, detected at runtime. All are
generic (D5).

| Primitive | Used by | Status |
|---|---|---|
| `PT_SET_SC_RET` (request 45: skip syscall, set result) | `renux-setscret` backend | Implemented, verified on 15.1-RELEASE-p6 |
| `trapvec`: syscall redirection to a supervisor through shared memory | `trapvec` backend | Design (M6) |

## 15. Testing strategy

| Level | What | Where |
|---|---|---|
| Unit | ABI layout, ELF parsing, config validation, vhost-user framing | `cargo test` |
| ABI conformance | A tiny freestanding **test kernel** (Rust, `no_std`) that exercises every `host_ops` entry and checks results | `tests/abi-kernel/` |
| Trap backend | Run static test programs in a guest space; verify syscall interception, faults, signals | `tests/trap/` |
| Boot | Boot `liu_defconfig` to a busybox initramfs, run a scripted shell session | `cargo xtask test-boot` |
| Compatibility | Selected Linux Test Project (LTP) syscall suites inside the guest | nightly CI |
| ABI drift | Regenerate `host.h`, fail on diff; compile-time `size_of`/`offset_of` assertions on both sides | CI |

The test kernel is important: it lets the runtime be developed and verified
without building Linux, which takes hours on the development machine.

## 16. Roadmap

| Milestone | Deliverable | Exit criterion |
|---|---|---|
| **M0 — Foundation** | Workspace, `liu-abi`, `liu-elf`, `liu-platform` (traits, Renux/FreeBSD and Linux shared memory, probing, Renux `PT_SET_SC_RET` primitive), style checker, CI | `cargo xtask ci` passes on Renux; workspace type-checks for Linux; `host.h` generated and checked |
| **M1 — Runtime + test kernel** | `liu-runtime` loads a freestanding `no_std` test kernel and serves `log`, `memory`, `time`, `cpu` | ABI conformance suite passes on Renux and Linux |
| **M2 — Guest spaces** | Trap backends `renux-setscret`, `freebsd-rewrite`, `linux-sysemu`; stub; physmem mappings | A static test binary runs in a guest space on each Tier 1 host; every syscall is trapped and answered by the test kernel |
| **M3 — Linux boots** | `kernel/` patch series, freestanding `arch/um` build, `os-liu` | `init=/bin/sh` from a busybox initramfs; `uname -a` and `cat /proc/version` work |
| **M4 — Devices** | `liu-vhost`, console, blk, rng | Root filesystem on `/dev/vda`; interactive console on `hvc0` |
| **M5 — Usable system** | net, fs, supervisor CLI, Capsicum everywhere | Fedora or Alpine userland boots; package manager installs over the network |
| **M6 — Performance** | `trapvec` Renux primitive and backend; `kvm` backend; SMP investigation | Measurable syscall latency improvement over the ptrace backends; published benchmarks |
| **M7 — More hosts** | Tier 2 platforms: NetBSD, DragonFly, macOS (`hvf`); display bridge (virtio-vsock + waypipe) for GUI and Flatpak apps | Boot test passes on every Tier 2 host; a Flatpak GUI app shows a window on the host |
| **M8 — arm64** | arm64 `GuestRegs`, `arch/um` arm64 port investigation | Feasibility report |

## 17. Licensing

- All code authored by LIU is **MPL-2.0**, matching Renux. Every Rust file
  carries `// SPDX-License-Identifier: MPL-2.0`.
- The Linux kernel and every file under `kernel/` (patches and `os-liu`) are
  **GPL-2.0-only**, because they are derived from or compiled into Linux.
- The generated header `include/liu/host.h` is dual-licensed
  `MPL-2.0 OR GPL-2.0-only` so it can be included on both sides.
- LIU code is not marked "Incompatible With Secondary Licenses". MPL-2.0 §3.3
  therefore permits distributing the runtime combined with the GPL-2.0 kernel
  image; the LIU source files remain available under MPL-2.0.
- Device backends are separate programs that communicate over vhost-user and
  remain purely MPL-2.0.

This section describes intent, not legal advice; it should be reviewed before
the first binary release.

## 18. Risks and open questions

| Risk | Mitigation |
|---|---|
| Freestanding `arch/um` build needs more kernel patching than expected | M1/M2 do not depend on Linux; measure the patch size in M3 and fall back to a static link of the runtime into the kernel if needed |
| ptrace trap cost makes syscall-heavy workloads slow | Batching in the stub; `trapvec` in M6; keep the backend trait stable |
| Signal-based interrupt delivery interacting with Rust code on the vCPU thread | Signal handlers only set state and enter Linux; no allocation, no locks; tested by the ABI conformance kernel |
| Upstream `arch/um` changes between LTS releases | Pin one LTS; CI rebases the series on point releases |
| `virtio_uml` expects Linux `eventfd` semantics for kicks | `eventfd` where available, a pipe pair elsewhere, behind `liu-platform` |
| A host's trap mechanism behaves differently from the others | Each backend has its own conformance tests; the trap test suite runs on every Tier 1 host in CI |
| Portability work slows down the Renux path | Renux is the reference platform; Tier 2 hosts never block a Tier 1 change |
| glibc vs musl in guest userland | Irrelevant to LIU: the guest runs a real Linux kernel; any distribution works |

Open questions:

1. 32-bit (i386) guest support: out of scope until M5.
2. SMP: one vCPU thread per Linux CPU requires `arch/um` SMP support in the
   pinned kernel; evaluate after M5.
3. Graphics: the M7 display bridge (virtio-vsock + waypipe) gives software-rendered windows; `virtio-gpu` acceleration is open.
