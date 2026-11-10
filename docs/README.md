# LIU documentation

| Document | What it covers |
|---|---|
| [Architecture](architecture.md) | How LIU works: execution model, host ABI, platform layer, devices, security, roadmap |
| [Decision records](adr/) | Why LIU is built the way it is, one decision per record |
| [Rust style](style.md) | The KNF rules every Rust file follows, checked by `cargo xtask style` |
| [Contributing](../CONTRIBUTING.md) | Building, checking and submitting changes |

## Reading order

1. The [README](../README.md) for the idea in one page.
2. [Architecture §1–§5](architecture.md) for how a Linux system call travels.
3. [ADR 0002](adr/0002-freestanding-kernel-rust-runtime.md) and
   [ADR 0006](adr/0006-portable-platform-layer.md), the two decisions that shape
   the code most.
4. [Architecture §16](architecture.md#16-roadmap) for what comes next.
