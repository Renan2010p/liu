/* SPDX-License-Identifier: MPL-2.0 OR GPL-2.0-only */

#ifndef LIU_HOST_H
#define LIU_HOST_H

/*
 * Generated from crates/liu-abi by `cargo xtask gen-header`.
 * DO NOT EDIT. Change the Rust definitions and regenerate.
 */

#include <stddef.h>
#include <stdint.h>

/**
 * Incompatible layout or semantic changes increment the major version.
 */
#define LIU_ABI_VERSION_MAJOR 0

/**
 * Additive changes (new table entries appended at the end) increment the
 * minor version.
 */
#define LIU_ABI_VERSION_MINOR 2

/**
 * `b"LIU\0"` read as a little-endian `u32`.
 */
#define LIU_ABI_MAGIC 5589324

/**
 * Memory protection: readable.
 */
#define LIU_PROT_READ 1

/**
 * Memory protection: writable.
 */
#define LIU_PROT_WRITE 2

/**
 * Memory protection: executable.
 */
#define LIU_PROT_EXEC 4

/**
 * All valid protection bits.
 */
#define LIU_PROT_MASK ((LIU_PROT_READ | LIU_PROT_WRITE) | LIU_PROT_EXEC)

/**
 * Page size assumed by the version 0 ABI.
 */
#define LIU_PAGE_SIZE 4096

/**
 * Number of entries in a version 0.1 `HostOps`.
 */
#define LIU_HOST_OPS_ENTRIES 33

/**
 * Number of entries in a version 0.1 `KernelOps`.
 */
#define LIU_KERNEL_OPS_ENTRIES 4

/**
 * Shutdown reason: power off.
 */
#define LIU_SHUTDOWN_POWEROFF 1

/**
 * Shutdown reason: reboot.
 */
#define LIU_SHUTDOWN_REBOOT 2

/**
 * The guest executed a system call instruction. The call was not executed
 * by the host. `TrapEvent::code` holds the call number from `rax` at entry.
 */
#define LIU_TRAP_SYSCALL 1

/**
 * The guest faulted on a memory access. `addr` and `access` describe it.
 */
#define LIU_TRAP_PAGE_FAULT 2

/**
 * The guest received a host signal that is not a page fault (illegal
 * instruction, breakpoint, arithmetic fault). `signo` and `code` hold the
 * architectural reason.
 */
#define LIU_TRAP_SIGNAL 3

/**
 * The guest address space no longer exists (the host process died).
 */
#define LIU_TRAP_EXITED 4

/**
 * Access bit: the faulting access was a read.
 */
#define LIU_ACCESS_READ 1

/**
 * Access bit: the faulting access was a write.
 */
#define LIU_ACCESS_WRITE 2

/**
 * Access bit: the faulting access was an instruction fetch.
 */
#define LIU_ACCESS_EXEC 4

/**
 * Signal numbers carried in `TrapEvent::signo`. These are LIU's own
 * numbers; neither host nor Linux numbering crosses the boundary.
 */
#define LIU_TRAP_SIG_ILL 1

#define LIU_TRAP_SIG_TRAP 2

#define LIU_TRAP_SIG_FPE 3

#define LIU_TRAP_SIG_BUS 4

/**
 * Opaque reference to a host object (address space, channel, event).
 *
 * Handles are allocated by the runtime. Their numeric value carries no
 * meaning on the kernel side and is never a host file descriptor.
 */
typedef uint64_t liu_handle;

/**
 * Raw status code as it crosses the boundary.
 */
typedef int32_t liu_status;

/**
 * First member of every boundary table.
 *
 * `size` is the size of the whole table as the producer compiled it. A
 * consumer built against a newer minor version treats every entry that lies
 * beyond `size` as absent, which is what makes appending entries a
 * compatible change.
 */
typedef struct liu_table_header {
  uint32_t magic;
  uint16_t abi_major;
  uint16_t abi_minor;
  uint32_t size;
  uint32_t reserved;
} liu_table_header;

/**
 * x86_64 user-mode register state of a guest thread.
 *
 * This is the architectural register set, not a Linux structure. The
 * runtime fills it from the host on every trap and loads it back before
 * resuming the guest.
 */
typedef struct liu_guest_regs {
  uint64_t rax;
  uint64_t rbx;
  uint64_t rcx;
  uint64_t rdx;
  uint64_t rsi;
  uint64_t rdi;
  uint64_t rbp;
  uint64_t rsp;
  uint64_t r8;
  uint64_t r9;
  uint64_t r10;
  uint64_t r11;
  uint64_t r12;
  uint64_t r13;
  uint64_t r14;
  uint64_t r15;
  uint64_t rip;
  uint64_t rflags;
  uint64_t fs_base;
  uint64_t gs_base;
} liu_guest_regs;

/**
 * Written by the runtime when `as_run` returns.
 */
typedef struct liu_trap_event {
  /**
   * One of the `TRAP_*` kinds.
   */
  uint32_t kind;
  /**
   * `TRAP_SIG_*` for `TRAP_SIGNAL`, zero otherwise.
   */
  uint32_t signo;
  /**
   * `ACCESS_*` bits for `TRAP_PAGE_FAULT`, zero otherwise.
   */
  uint32_t access;
  uint32_t reserved;
  /**
   * Faulting address for `TRAP_PAGE_FAULT`.
   */
  uint64_t addr;
  /**
   * System call number for `TRAP_SYSCALL`, exit status for `TRAP_EXITED`.
   */
  uint64_t code;
} liu_trap_event;

typedef struct liu_host_ops {
  struct liu_table_header header;
  /**
   * Writes early boot output. May be called from any context.
   */
  void (*log_write)(const uint8_t *buf, size_t len);
  /**
   * Stops the instance after a fatal kernel error. Does not return.
   */
  void (*panic)(const uint8_t *msg, size_t len);
  /**
   * Maps `len` bytes of physical memory at `phys` to kernel address
   * `vaddr`, replacing any previous mapping.
   *
   * The range must lie in the `kmap` window of `BootInfo`, and every
   * argument must be page aligned (`STATUS_INVALID`). A range outside the
   * window or beyond physical memory is `STATUS_FAULT`; writable and
   * executable at once is `STATUS_DENIED`.
   */
  liu_status (*kmap)(uint64_t vaddr, uint64_t phys, uint64_t len, uint32_t prot);
  /**
   * Makes a window range inaccessible again. Unmapped pages are ignored.
   */
  liu_status (*kunmap)(uint64_t vaddr, uint64_t len);
  /**
   * Changes the protection of mapped window pages; any unmapped page in
   * the range is `STATUS_FAULT`. Same rules as `kmap` otherwise.
   */
  liu_status (*kprotect)(uint64_t vaddr, uint64_t len, uint32_t prot);
  liu_status (*as_create)(liu_handle *out);
  liu_status (*as_destroy)(liu_handle space);
  liu_status (*as_map)(liu_handle space, uint64_t vaddr, uint64_t phys, uint64_t len, uint32_t prot);
  liu_status (*as_unmap)(liu_handle space, uint64_t vaddr, uint64_t len);
  liu_status (*as_protect)(liu_handle space, uint64_t vaddr, uint64_t len, uint32_t prot);
  liu_status (*as_flush)(liu_handle space);
  /**
   * Loads `regs`, runs guest code in `space` until the next trap, stores
   * the resulting registers back into `regs` and describes the trap in
   * `event`.
   */
  liu_status (*as_run)(liu_handle space, struct liu_guest_regs *regs, struct liu_trap_event *event);
  liu_status (*as_get_fpu)(liu_handle space, uint8_t *buf, size_t len);
  liu_status (*as_set_fpu)(liu_handle space, const uint8_t *buf, size_t len);
  void (*irq_enable)(void);
  void (*irq_disable)(void);
  /**
   * Disables interrupts and returns the previous state.
   */
  uint64_t (*irq_save)(void);
  void (*irq_restore)(uint64_t flags);
  /**
   * Blocks until an interrupt is pending. Returns with interrupts enabled.
   */
  void (*idle)(void);
  uint64_t (*clock_monotonic)(void);
  uint64_t (*clock_realtime)(void);
  /**
   * Arms the one-shot timer at an absolute monotonic deadline.
   */
  liu_status (*timer_set)(uint64_t deadline);
  void (*timer_cancel)(void);
  /**
   * Allocates a zero-filled stack of `size` bytes (a page multiple) with
   * an inaccessible guard page below it, and stores its lowest usable
   * address in `out`. The stack grows down from `*out + size`.
   */
  liu_status (*kstack_alloc)(uint64_t size, uint64_t *out);
  /**
   * Frees a stack returned by `kstack_alloc`, given the same address and
   * size. Anything else is ignored.
   */
  void (*kstack_free)(uint64_t addr, uint64_t size);
  liu_status (*get_random)(uint8_t *buf, size_t len);
  liu_status (*dev_connect)(const uint8_t *name, size_t len, liu_handle *out);
  liu_status (*chan_send)(liu_handle chan,
                          const uint8_t *buf,
                          size_t len,
                          const liu_handle *handles,
                          size_t nhandles);
  liu_status (*chan_recv)(liu_handle chan,
                          uint8_t *buf,
                          size_t cap,
                          size_t *out_len,
                          liu_handle *handles,
                          size_t handles_cap,
                          size_t *out_nhandles);
  liu_status (*event_create)(liu_handle *out);
  liu_status (*event_signal)(liu_handle event);
  /**
   * Raises interrupt `irq` whenever `handle` becomes readable.
   */
  liu_status (*irq_bind)(liu_handle handle, uint32_t irq);
  liu_status (*handle_close)(liu_handle handle);
} liu_host_ops;

/**
 * Filled in by the kernel during `liu_kernel_entry`, before it enables
 * interrupts. Every entry is called on the vCPU thread.
 */
typedef struct liu_kernel_ops {
  struct liu_table_header header;
  /**
   * Delivers the interrupts whose bits are set in `pending`.
   */
  void (*irq)(uint64_t pending);
  /**
   * The one-shot timer armed with `timer_set` expired.
   */
  void (*timer)(void);
  /**
   * A fault occurred while the vCPU was executing kernel code.
   */
  liu_status (*kernel_fault)(uint64_t addr, uint32_t access, uint64_t ip);
  /**
   * The supervisor requests an orderly shutdown.
   */
  void (*shutdown)(uint32_t reason);
} liu_kernel_ops;

/**
 * Boot parameters, valid for the lifetime of the kernel.
 *
 * All pointers are addresses in the kernel process. Guest physical memory is
 * mapped contiguously at `physmem_base` before entry.
 */
typedef struct liu_boot_info {
  struct liu_table_header header;
  /**
   * Kernel virtual address where guest physical memory starts.
   */
  uint64_t physmem_base;
  /**
   * Size of guest physical memory in bytes, a multiple of the page size.
   */
  uint64_t physmem_size;
  /**
   * Handle of the physical memory object, passed to device backends.
   */
  liu_handle physmem_handle;
  /**
   * Lowest address of the loaded kernel image.
   */
  uint64_t image_base;
  /**
   * Size of the loaded kernel image, page aligned.
   */
  uint64_t image_size;
  /**
   * Kernel command line, not NUL terminated.
   */
  const uint8_t *cmdline;
  uint64_t cmdline_len;
  /**
   * Offset of the initial ramdisk in physical memory, or zero.
   */
  uint64_t initrd_offset;
  /**
   * Size of the initial ramdisk, or zero when absent.
   */
  uint64_t initrd_size;
  /**
   * Size of the buffer `as_get_fpu` and `as_set_fpu` expect.
   */
  uint64_t fpu_state_size;
  /**
   * Host page size. Version 0 requires `PAGE_SIZE`.
   */
  uint64_t page_size;
  /**
   * Start of the window where `kmap` may map physical memory (ABI 0.2).
   * The window is page aligned and inaccessible until mapped.
   */
  uint64_t kmap_base;
  /**
   * Size of the `kmap` window in bytes (ABI 0.2).
   */
  uint64_t kmap_size;
} liu_boot_info;

/**
 * The value of a handle that refers to nothing.
 */
#define LIU_HANDLE_INVALID UINT64_MAX

/**
 * The operation succeeded.
 */
#define LIU_STATUS_OK 0

/**
 * An argument was invalid (bad range, bad flags, misaligned address).
 */
#define LIU_STATUS_INVALID 1

/**
 * The host could not allocate memory or another resource.
 */
#define LIU_STATUS_NO_MEMORY 2

/**
 * A handle did not refer to a live object of the expected kind.
 */
#define LIU_STATUS_BAD_HANDLE 3

/**
 * The operation is not provided by this runtime.
 */
#define LIU_STATUS_UNSUPPORTED 4

/**
 * The object is busy; retry later.
 */
#define LIU_STATUS_BUSY 5

/**
 * A pointer passed to the host was not accessible.
 */
#define LIU_STATUS_FAULT 6

/**
 * A host I/O error occurred.
 */
#define LIU_STATUS_IO 7

/**
 * The operation would block.
 */
#define LIU_STATUS_AGAIN 8

/**
 * The operation was denied by policy (Capsicum, broker).
 */
#define LIU_STATUS_DENIED 9

/**
 * The peer speaks an incompatible ABI version.
 */
#define LIU_STATUS_VERSION 10

/**
 * The named object does not exist.
 */
#define LIU_STATUS_NOT_FOUND 11

#endif  /* LIU_HOST_H */

/* ELF entry point of the kernel image; see KernelEntry in liu-abi. */
liu_status liu_kernel_entry(const struct liu_host_ops *host,
                            struct liu_kernel_ops *kernel,
                            const struct liu_boot_info *boot);
