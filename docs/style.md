# Rust style (KNF)

LIU's Rust code follows an adaptation of FreeBSD's kernel normal form,
`style(9)`. Everything is explicit: return types, return statements, binding
types and closure signatures.

`rustfmt` cannot produce this style. **Do not run `cargo fmt`.** The rules
below are enforced by `cargo xtask style`, which is part of `cargo xtask ci`.

## Rules

1. **Function name on its own line.** `fn` and its qualifiers (`pub`, `const`,
   `unsafe`, `extern "C"`) go on one line; the name and signature start the next.
2. **Braces on their own line.** Every `{` that ends a line stands alone on that
   line: functions, `impl`, `struct`, `enum`, `mod`, `if`/`else`, `match` and its
   arms, loops, `unsafe` blocks, closures and struct literals. `else` starts its
   own line.
3. **Explicit return type.** Every function and function pointer type declares
   `->`, including `-> ()`. Diverging functions use `-> !`.
4. **Explicit return.** Values are returned with `return (expr);`, never as a
   trailing expression. A `-> ()` function may use a bare `return;` to leave early.
5. **Typed bindings.** Every `let` has a type annotation, including patterns:
   `let (a, b): (u32, u32) = ...;`. `if let` and `while let` are exempt because
   Rust does not allow annotations there.
6. **Typed closures.** Closures type every parameter and declare `->`:
   `|x: u32| -> u32 { return (x + 1); }`.
7. **Single-line blocks.** A block that opens and closes on the same line is
   allowed for short expressions: `unsafe { libc::getpid() }`,
   `if ok { 42 } else { 1 }`, struct literals such as `Foo { a: 1 }`.

## Example

```rust
/// Returns the page-aligned end of a segment.
pub fn
segment_end(seg: &Segment) -> Result<u64, ImageError>
{
    let end: u64 = seg.vaddr.checked_add(seg.memsz).ok_or(ImageError::AddressOverflow)?;

    if end == 0
    {
        return (Err(ImageError::AddressOverflow));
    }
    else
    {
        return (Ok(align_up(end)));
    }
}

impl Drop for Mapping
{
    fn
    drop(&mut self) -> ()
    {
        // SAFETY: `base` and `len` describe a mapping this value owns.
        unsafe
        {
            libc::munmap(self.base.as_ptr().cast(), self.len);
        }
    }
}
```

## Other conventions

- Every file starts with `// SPDX-License-Identifier: MPL-2.0`.
- Declarations come first in a block, followed by a blank line.
- Every `unsafe` block carries a `// SAFETY:` comment.
- `unsafe` is allowed only in `liu-platform`, `liu-physmem`, `liu-trap` and the
  `host_ops` glue of `liu-runtime`; other crates use `#![forbid(unsafe_code)]`.
- `#[cfg(target_os = ...)]` is allowed only in `liu-platform`.
- Lines are at most 100 columns where practical.

The workspace disables the lints that contradict KNF (`unused_parens`,
`clippy::needless_return`, `clippy::unused_unit`, `clippy::let_and_return`).
