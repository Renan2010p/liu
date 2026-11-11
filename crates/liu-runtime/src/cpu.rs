// SPDX-License-Identifier: MPL-2.0

//! The virtual CPU's interrupt state and one-shot timer.
//!
//! Interrupts are delivered only at safe points: when the kernel enables
//! interrupts (`irq_enable`, `irq_restore`) and when it returns from `idle`.
//! At each safe point an expired timer deadline and the pending interrupt
//! lines are collected, interrupts are disabled as hardware would, and the
//! kernel's handlers run. Asynchronous preemption by a host signal is left
//! to M3, when Linux needs it.
//!
//! Handlers never nest: a safe point reached from inside a handler only
//! updates the enable flag.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// What is due at a safe point.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Delivery
{
    /// The one-shot timer expired.
    pub timer: bool,
    /// Pending interrupt lines, one bit each.
    pub irqs: u64,
}

impl Delivery
{
    /// Returns whether nothing is due.
    pub fn
    is_empty(&self) -> bool
    {
        return (!self.timer && self.irqs == 0);
    }
}

/// Interrupt and timer state of one vCPU.
#[derive(Debug)]
pub struct Cpu
{
    enabled: AtomicBool,
    delivering: AtomicBool,
    pending: AtomicU64,
    /// Absolute monotonic deadline in nanoseconds; 0 when disarmed.
    deadline: AtomicU64,
    lock: Mutex<()>,
    wake: Condvar,
}

impl Default for Cpu
{
    fn
    default() -> Cpu
    {
        return (Cpu::new());
    }
}

impl Cpu
{
    /// A vCPU with interrupts disabled, nothing pending and no timer.
    pub const fn
    new() -> Cpu
    {
        let cpu: Cpu = Cpu
        {
            enabled: AtomicBool::new(false),
            delivering: AtomicBool::new(false),
            pending: AtomicU64::new(0),
            deadline: AtomicU64::new(0),
            lock: Mutex::new(()),
            wake: Condvar::new(),
        };

        return (cpu);
    }

    /// Returns to the state at kernel entry.
    pub fn
    reset(&self) -> ()
    {
        self.enabled.store(false, Ordering::SeqCst);
        self.delivering.store(false, Ordering::SeqCst);
        self.pending.store(0, Ordering::SeqCst);
        self.deadline.store(0, Ordering::SeqCst);
    }

    fn
    guard(&self) -> MutexGuard<'_, ()>
    {
        return (self.lock.lock().unwrap_or_else(PoisonError::into_inner));
    }

    /// Wakes a vCPU blocked in [`Cpu::wait`] so it re-examines its state.
    fn
    notify(&self) -> ()
    {
        let _guard: MutexGuard<'_, ()> = self.guard();

        self.wake.notify_all();
    }

    /// Returns whether interrupts are enabled.
    pub fn
    enabled(&self) -> bool
    {
        return (self.enabled.load(Ordering::SeqCst));
    }

    /// Sets the enable flag and returns the previous one. Does not deliver.
    pub fn
    set_enabled(&self, on: bool) -> bool
    {
        return (self.enabled.swap(on, Ordering::SeqCst));
    }

    /// Marks interrupt lines pending. Callable from any thread.
    pub fn
    raise(&self, lines: u64) -> ()
    {
        self.pending.fetch_or(lines, Ordering::SeqCst);
        self.notify();
    }

    /// Arms the timer at an absolute monotonic deadline, replacing any
    /// previous one. A deadline in the past fires at the next safe point.
    pub fn
    set_timer(&self, deadline: u64) -> ()
    {
        self.deadline.store(deadline.max(1), Ordering::SeqCst);
        self.notify();
    }

    /// Disarms the timer.
    pub fn
    cancel_timer(&self) -> ()
    {
        self.deadline.store(0, Ordering::SeqCst);
    }

    /// Returns whether something is due at time `now`.
    pub fn
    has_work(&self, now: u64) -> bool
    {
        let deadline: u64 = self.deadline.load(Ordering::SeqCst);

        return (self.pending.load(Ordering::SeqCst) != 0 || (deadline != 0 && now >= deadline));
    }

    /// Collects what is due at time `now` and clears it.
    fn
    take(&self, now: u64) -> Delivery
    {
        let deadline: u64 = self.deadline.load(Ordering::SeqCst);
        let timer: bool = deadline != 0
            && now >= deadline
            && self.deadline.compare_exchange(deadline, 0, Ordering::SeqCst, Ordering::SeqCst).is_ok();
        let irqs: u64 = self.pending.swap(0, Ordering::SeqCst);

        return (Delivery { timer: timer, irqs: irqs });
    }

    /// Safe point: if interrupts are enabled and no handler is running, runs
    /// `handler` with interrupts disabled until nothing is due.
    pub fn
    deliver(&self, now: &dyn Fn() -> u64, handler: &mut dyn FnMut(Delivery) -> ()) -> ()
    {
        if !self.enabled() || self.delivering.swap(true, Ordering::SeqCst)
        {
            return;
        }

        loop
        {
            let due: Delivery = self.take(now());

            if due.is_empty()
            {
                break;
            }
            self.set_enabled(false);
            handler(due);
            self.set_enabled(true);
        }

        self.delivering.store(false, Ordering::SeqCst);
    }

    /// Blocks until something is due. Without a timer or another thread
    /// raising a line, this blocks forever, as a halted CPU would.
    pub fn
    wait(&self, now: &dyn Fn() -> u64) -> ()
    {
        let mut guard: MutexGuard<'_, ()> = self.guard();

        loop
        {
            let time: u64 = now();

            if self.has_work(time)
            {
                return;
            }

            let deadline: u64 = self.deadline.load(Ordering::SeqCst);

            guard = if deadline == 0
            {
                self.wake.wait(guard).unwrap_or_else(PoisonError::into_inner)
            }
            else
            {
                let timeout: Duration = Duration::from_nanos(deadline - time);

                self.wake.wait_timeout(guard, timeout).unwrap_or_else(PoisonError::into_inner).0
            };
        }
    }
}

#[cfg(test)]
mod tests
{
    use super::*;
    use std::sync::Arc;
    use std::thread;

    fn
    at(time: u64) -> impl Fn() -> u64
    {
        return (move || -> u64 { return (time); });
    }

    fn
    collect(cpu: &Cpu, time: u64) -> Vec<Delivery>
    {
        let mut seen: Vec<Delivery> = Vec::new();

        cpu.deliver(&at(time), &mut |due: Delivery| -> ()
        {
            seen.push(due);
        });

        return (seen);
    }

    #[test]
    fn
    nothing_is_delivered_while_disabled() -> ()
    {
        let cpu: Cpu = Cpu::new();

        cpu.set_timer(10);
        cpu.raise(0b1);
        assert!(collect(&cpu, 20).is_empty());
        assert!(cpu.has_work(20));
    }

    #[test]
    fn
    expired_timer_and_lines_are_delivered_once() -> ()
    {
        let cpu: Cpu = Cpu::new();

        cpu.set_enabled(true);
        cpu.set_timer(10);
        cpu.raise(0b101);
        assert_eq!(collect(&cpu, 20), vec![Delivery { timer: true, irqs: 0b101 }]);
        assert!(collect(&cpu, 30).is_empty());
        assert!(cpu.enabled());
    }

    #[test]
    fn
    future_timer_is_not_delivered() -> ()
    {
        let cpu: Cpu = Cpu::new();

        cpu.set_enabled(true);
        cpu.set_timer(100);
        assert!(collect(&cpu, 99).is_empty());
        assert_eq!(collect(&cpu, 100), vec![Delivery { timer: true, irqs: 0 }]);
    }

    #[test]
    fn
    cancelled_timer_never_fires() -> ()
    {
        let cpu: Cpu = Cpu::new();

        cpu.set_enabled(true);
        cpu.set_timer(10);
        cpu.cancel_timer();
        assert!(collect(&cpu, 1000).is_empty());
    }

    #[test]
    fn
    handlers_run_disabled_and_do_not_nest() -> ()
    {
        let cpu: Cpu = Cpu::new();
        let mut calls: u32 = 0;

        cpu.set_enabled(true);
        cpu.set_timer(1);
        cpu.deliver(&at(5), &mut |_: Delivery| -> ()
        {
            calls += 1;
            assert!(!cpu.enabled());
            cpu.set_enabled(true);
            if calls == 1
            {
                cpu.raise(0b10);
            }
            cpu.deliver(&at(5), &mut |_: Delivery| -> ()
            {
                panic!("nested delivery");
            });
        });
        // The line raised inside the handler is delivered by the same loop.
        assert_eq!(calls, 2);
    }

    #[test]
    fn
    wait_returns_at_the_deadline() -> ()
    {
        let cpu: Cpu = Cpu::new();
        let now: &dyn Fn() -> u64 = &liu_platform::clock_monotonic_ns;
        let start: u64 = now();

        cpu.set_timer(start + 5_000_000);
        cpu.wait(now);
        assert!(now() >= start + 5_000_000);
    }

    #[test]
    fn
    wait_wakes_on_raise_from_another_thread() -> ()
    {
        let cpu: Arc<Cpu> = Arc::new(Cpu::new());
        let other: Arc<Cpu> = Arc::clone(&cpu);
        let raiser: thread::JoinHandle<()> = thread::spawn(move || -> ()
        {
            thread::sleep(Duration::from_millis(5));
            other.raise(0b1);
        });

        cpu.wait(&liu_platform::clock_monotonic_ns);
        assert!(cpu.has_work(0));
        raiser.join().unwrap();
    }
}
