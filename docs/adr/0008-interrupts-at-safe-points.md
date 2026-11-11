# ADR 0008: Interrupts delivered at safe points until M3

## Status

Proposed

## Context

The kernel runs uniprocessor on one vCPU thread (§5.5). Host events (the
one-shot timer now, device kicks from M4) must reach `kernel_ops.timer()` and
`kernel_ops.irq()`. Real hardware, and UML with `SIGALRM`/`SIGIO`, interrupt
the kernel asynchronously at any instruction. Doing that from a host signal
handler means running kernel code on a signal stack, with every host-side
lock and allocation in the runtime becoming a hazard.

M1 has only a test kernel, and Linux itself does not run before M3.

## Decision

Until M3, interrupts are delivered only at **safe points**:

- `irq_enable`, and `irq_restore` with interrupts enabled;
- the return from `idle`, which blocks until something is due.

At a safe point the runtime collects an expired timer deadline and the
pending interrupt lines, disables interrupts as hardware would, runs the
kernel's handlers, and enables them again. Handlers never nest. The timer is
a deadline checked against the host monotonic clock, so no event thread is
needed yet; `idle` sleeps until the deadline. Another thread can still raise
lines (`Cpu::raise`), which the M4 event thread will use.

## Consequences

- Delivery is synchronous and deterministic, and the interrupt logic is
  plain safe Rust with unit tests (`liu-runtime/src/cpu.rs`).
- A kernel that loops with interrupts disabled, or never reaches a safe
  point, is never interrupted. Linux reaches safe points constantly
  (`local_irq_enable`, `spin_unlock_irqrestore`), so this is acceptable for
  bring-up, but preemption of long user-triggered loops needs M3's
  asynchronous path.
- Every safe point reads the monotonic clock (a vDSO call on Linux and
  FreeBSD).

## Alternatives considered

- **Asynchronous delivery by host signal now.** Deferred to M3, where it is
  needed and where the trap backends define how the vCPU thread is
  interrupted while a guest space runs.
- **An event thread with kqueue/epoll for the timer.** Not needed for one
  timer; it arrives with device file descriptors in M4.
