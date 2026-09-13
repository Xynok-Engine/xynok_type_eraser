---
title: What is memory alignment?
excerpt: What is memory alignment, and why does it matter?
cover img: "../images/async_await.png"
tags:
  - bit
  - addendum
---
# Alignment (Memory alignment)

This note explains what alignment is, why it exists, and why `InlineFn` needs to care about it while standard Rust code generally does not.

## Memory isn't as flat as you think

Logically, RAM is a sequence of bytes, each with an address: 0, 1, 2, 3...

However, the CPU does not read one byte at a time. It reads in blocks, typically 8 bytes at once on 64-bit machines. These blocks are fixed, starting at addresses 0, 8, 16, 24... Think of it like a cabinet with pre-divided shelves, each holding 8 slots. You can only open a whole shelf, not half of one.

Reading a `u64` (8 bytes) located at address 8:

```
address:  0  1  2  3  4  5  6  7 | 8  9 10 11 12 13 14 15
block:    [------- block 0 -----] [------- block 1 -----]
u64:                               ^^^^^^^^^^^^^^^^^^^^^^
```

It sits neatly within block 1, so the CPU loads one block and is done.

The same `u64` but at address 5:

```
address:  0  1  2  3  4  5  6  7 | 8  9 10 11 12 13 14 15
block:    [------- block 0 -----] [------- block 1 -----]
u64:                     ^^^^^^^^^^^^^^^^^^^^
```

It straddles two blocks. The CPU must load block 0, load block 1, then stitch them together. This is slower, and on some architectures, it is simply impossible, causing the hardware to trigger an error.

## Definition

`align_of::<T>()` answers the question: "What must the address of a value of type `T` be divisible by?"

On a 64-bit machine:

| Type | size | align | address must be divisible by |
| --- | --- | --- | --- |
| `u8`, `bool` | 1 | 1 | 1 (can be anywhere) |
| `u16` | 2 | 2 | 2 |
| `u32`, `char` | 4 | 4 | 4 |
| `u64`, `usize`, all pointers | 8 | 8 | 8 |
| `u128` | 16 | 16 | 16 |

A `u64` at address 5 is misaligned, because 5 is not divisible by 8. At addresses 8, 16, or 24, it is fine.

For a struct, the alignment is equal to the largest alignment among its fields:

```rust
struct Foo
{
    a: u8,  // align 1
    b: u64, // align 8
}
// align_of::<Foo>() == 8
```

## Padding: where alignment shows itself

The `Foo` struct above looks like it only needs 9 bytes, but `size_of::<Foo>()` is 16:

```
offset:  0  1  2  3  4  5  6  7  8 ...    15
         a  ▓  ▓  ▓  ▓  ▓  ▓  ▓  [---- b ----]
            └──── 7 bytes of padding ────┘
```

`b` cannot reside at offset 1. If `Foo` is placed at an address divisible by 8, `b` would fall at an address that, when divided by 8, leaves a remainder of 1, which is misaligned. The compiler inserts 7 empty bytes to push `b` to offset 8. Additionally, the size of a struct is always a multiple of its alignment, so the total is 16 rather than 9.

Rust is allowed to reorder fields to optimize this for you. C is not, which is why C programmers often pay close attention to the order of declaration.

## Why nobody usually thinks about this

Because the compiler and the standard library handle everything:

- `let x = 5u64;` causes the compiler to place `x` at an aligned address on the stack.
- `Box::new(v)` calls the allocator with the correct alignment for the type.
- `Vec<T>` allocates memory aligned according to `T`.

You only need to worry about this when managing raw memory yourself. That is exactly what `InlineFn` is doing.

## What happens if you ignore it

"But x86 reads and writes work even when misaligned, right?" True, most `mov` instructions on x86 handle misaligned addresses. This is why this type of bug often runs perfectly during development only to crash in production. There are three reasons you still shouldn't rely on this:

1. **UB is UB to the optimizer.** LLVM is allowed to assume a pointer is aligned and optimize based on that assumption. It could remove a check, merge two load instructions, or generate nonsense code in a completely unrelated area. This is not just a case of "running a bit slower."

2. **SIMD will actually break.** If data has an alignment of 16 and LLVM decides to copy it using `movaps`, a misaligned address will fault immediately, resulting in a segfault rather than incorrect data.

3. **ARM.** AArch64 can handle misalignment with standard load/store instructions, but atomic instructions (`ldxr`/`stxr`) require strict alignment. A closure capturing an `Arc` will increment or decrement the refcount using an atomic operation on that very pointer.

## How this relates to `InlineFn`

`InlineFn` stores a closure in a raw buffer instead of a `Box` to avoid heap allocation. That buffer is currently:

```rust
struct FnBuffer<const SIZE: usize>
{
    buffer: [MaybeUninit<u8>; SIZE],
}
```

It is a `u8` array, meaning the alignment is 1. In other words, we have just told the compiler that "this area can be placed anywhere, I don't need alignment." The compiler believes us and has the right to place it at any arbitrary address.

Then we write the closure into it:

```
buffer.as_mut_ptr().cast::<T>().write(f);
```

`ptr::write::<T>` has a mandatory requirement in its safety contract: the pointer must be aligned to `align_of::<T>()`. If the closure captures an `Arc`, it contains an 8-byte aligned pointer, but we are writing it into a region we promised only has 1-byte alignment. This violates the contract, which is UB, regardless of whether the buffer is large enough.

Therefore, `is_fit` must check two conditions, not just one:

```rust
fn is_fit<T: IRunnable, const S: usize>() -> bool
{
    size_of::<T>() <= S                             // does it fit?
        && align_of::<T>() <= align_of::<FnBuffer<S>>() // can it be placed correctly?
}
```

Size answers "does the closure fit in the buffer?" Alignment answers "can the buffer place the closure correctly?" These are two distinct questions, and both must be true to use the inline path. If either is false, `InlineFn` falls back to the boxed path, where the allocator ensures correct alignment for us.

## Current status and fix

Because `align_of::<FnBuffer<S>>()` is currently 1, the second condition is almost always false. The code isn't incorrect, but it is refusing to inline almost every closure that captures variables, meaning the fast path essentially does not exist.

The fix is to increase the promise of the buffer rather than relaxing `is_fit`:

```rust
#[repr(C, align(16))]
struct FnBuffer<const SIZE: usize>
{
    buffer: [MaybeUninit<u8>; SIZE],
}
```

Now we tell the compiler that this struct must always reside at an address divisible by 16, and the compiler ensures this wherever `FnBuffer` appears. Because 16 is divisible by 8, 4, 2, and 1, any closure capturing pointers, `u32`, `u64`, or `u128` can be placed there. `is_fit` remains unchanged, but now it passes.

Types requiring alignment greater than 16 (for example, SIMD `__m256` which requires 32) will still automatically fall back to the boxed path, just as we want. This is why you should keep the alignment condition in `is_fit` instead of removing it.

A small note: `align(16)` causes `size_of::<FnBuffer<S>>()` to be rounded up to a multiple of 16. With `SMALL = 32` and `LARGE = 64`, nothing changes, but `InlineFn<20>` will occupy 32 bytes instead of 20.

## Further reading

- [Type layout](https://doc.rust-lang.org/reference/type-layout.html) in the Rust Reference
- [`std::ptr::write`](https://doc.rust-lang.org/std/ptr/fn.write.html), under Safety
- [`std::mem::align_of`](https://doc.rust-lang.org/std/mem/fn.align_of.html)
- https://swiftunboxed.com/internals/size-stride-alignment/
