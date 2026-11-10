# ADR 0007: LIU as a Renux component, delivered as a package

## Status

Proposed

## Context

LIU was designed for Renux: its fastest trap backend uses a Renux kernel
primitive (`PT_SET_SC_RET`), and it is meant to become how Renux runs Linux
software. The question is where it lives: inside the Renux source tree, or
beside it.

Renux's [philosophy](https://github.com/renuxproject/renux/blob/main/PHILOSOPHY.md)
answers most of it:

- **"Zig is the only dependency."** The base system must build with Zig alone.
  LIU is written in Rust and needs a Rust toolchain.
- **"The whole system is the base system … packages are outside it."**
- Renux's work is MPL-2.0, and the base keeps FreeBSD's BSD code. A Linux kernel
  image is GPL-2.0.
- ADR 0005 already keeps Linux code out of the Renux kernel.
- ADR 0006 makes LIU portable to other hosts.

## Decision

LIU is an **official Renux component** in three parts:

1. **Renux base system: generic primitives only.** Kernel features LIU uses
   (`PT_SET_SC_RET`, later `trapvec`) are committed to the Renux tree as
   generic, documented and tested ptrace or process features, with no mention
   of Linux. They must be useful and testable without LIU.
2. **Official Renux packages.**
   - `liu`: runtime, device backends, CLI and an `rc.d` service
     (`liu_enable`, instances in `/usr/local/etc/liu/`).
   - `liu-kernel`: the Linux kernel image built from LIU's patch series, GPL-2.0,
     with its source offered alongside.
3. **One upstream.** LIU stays a separate repository that serves every host.
   It may move under the `renuxproject` organization; Renux remains its
   reference platform (Tier 1, release blocker).

## Consequences

- The Renux base stays Zig-only, free of Linux code and free of GPL code.
- Renux users install LIU with one command and get the fast path automatically.
- LIU's CI covers FreeBSD and Linux as well, which also protects Renux.
- `PT_SET_SC_RET` must be committed to the Renux tree with its tests; today it
  exists only in a local working tree.
- A Renux release can document LIU as its way to run Linux software. Whether the
  linuxulator (`COMPAT_LINUX`) is later dropped from Renux is a separate
  decision, to take once LIU reaches M5.

## Alternatives considered

- **Import LIU into the Renux tree (`contrib/liu`).** Rejected: breaks "Zig is
  the only dependency" and brings GPL code into the tree.
- **Rewrite LIU in Zig to fit the base.** Rejected for now: the Rust work is
  done and tested, and the base-system rule does not apply to packages. The
  language of new Renux code is still an open question in the philosophy.
- **Keep LIU unrelated to Renux.** Rejected: Renux is the platform it was
  designed for, and the kernel primitives live there.
