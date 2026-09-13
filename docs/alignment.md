---
title: Size, stride, and alignment
excerpt: How a type occupies memory and why raw storage must account for more than its byte count.
tags:
  - memory
  - addendum
---

# Size, stride, and alignment

Every value stored in memory has three related properties:

- **Size** is the number of bytes occupied by one value.
- **Stride** is the distance in bytes from the start of one value to the start of the next value in a contiguous sequence.
- **Alignment** is the address boundary on which a value must begin.

These properties are easy to ignore when using normal Rust values because the compiler, `Box`, and collections such as `Vec` handle them. They matter when implementing raw storage, allocating memory manually, exchanging data with another language, or calculating field offsets.

## Size

Rust reports the size of a type with `std::mem::size_of`:

```rust
use std::mem::size_of;

assert_eq!(size_of::<u8>(), 1);
assert_eq!(size_of::<u32>(), 4);
assert_eq!(size_of::<[u32; 2]>(), 8);
```

For a struct, size includes its fields and any padding inserted between or after them. Consider a type with a stable C-compatible field order:

```rust
#[repr(C)]
struct Puppy
{
    age:        u64,
    is_trained: bool,
}
```

The fields contain nine bytes of data: eight for `age` and one for `is_trained`. The struct normally occupies 16 bytes, however, because it needs seven trailing padding bytes:

```text
offset:  0 1 2 3 4 5 6 7  8  9 10 11 12 13 14 15
         [---- age ----]  T  ·  ·  ·  ·  ·  ·  ·
                              seven padding bytes
```

The exact values are target-dependent, so code can inspect them instead of assuming them:

```rust
assert_eq!(size_of::<Puppy>(), 16); // on common 64-bit targets
```

## Alignment

Alignment describes which addresses are valid for a type. `std::mem::align_of::<T>()` returns the required boundary. If a type has alignment 8, its address must be divisible by 8, such as 0, 8, 16, or 24.

```rust
use std::mem::align_of;

assert_eq!(align_of::<u8>(), 1);
assert_eq!(align_of::<u32>(), 4);
assert_eq!(align_of::<u64>(), 8); // on common 64-bit targets
```

The alignment of a C-layout struct is at least the largest alignment of its fields. `Puppy` therefore has alignment 8 on a target where `u64` has alignment 8.

Alignment affects both field placement and total size. In a contiguous array, every element must begin at a correctly aligned address. If `Puppy` used only its nine data bytes, the second value would start at address 9, which is invalid for its `u64` field. Padding rounds the element's occupied space up to 16 bytes, so subsequent elements begin at 16, 32, 48, and so on.

Misaligned typed pointer reads and writes are undefined behavior in Rust, even on hardware that can perform some unaligned operations. Raw-pointer functions such as `ptr::read_unaligned` exist for data that is intentionally unaligned, but they do not make an unaligned address suitable for storing and using an ordinary `T`.

## Stride

Stride answers a pointer-arithmetic question: how far must a pointer move to reach the next element?

```text
first value                              second value
┌─────────────────────────────────────┐  ┌────────────
│ 9 bytes of fields + 7 bytes padding │  │ ...
└─────────────────────────────────────┘  └────────────
^                                        ^
address 0                                address 16
                 stride = 16
```

Swift exposes size and stride separately: a value may have a size of 9 and a stride of 16 because Swift's size does not include trailing padding. Rust uses a different definition. `size_of::<T>()` includes trailing padding, and array elements are always `size_of::<T>()` bytes apart. Rust therefore has no separate `stride_of` function for ordinary sized types:

```rust
use std::mem::size_of;

let values = [
    Puppy { age: 1, is_trained: false },
    Puppy { age: 2, is_trained: true },
];

let first = values.as_ptr() as usize;
let second = unsafe { values.as_ptr().add(1) } as usize;

assert_eq!(second - first, size_of::<Puppy>());
```

For Rust arrays, the practical relationship is:

```text
stride(T) = size_of::<T>()
```

This also means `[T; N]` has size `size_of::<T>() * N`. The special case is a zero-sized type such as `()`: its size and array stride are zero even though Rust references still have alignment and validity requirements.

## Padding and field order

Padding can appear between fields as well as after the last field. With C layout, reversing the fields changes where the padding goes:

```rust
#[repr(C)]
struct AlternatePuppy
{
    is_trained: bool,
    age:        u64,
}
```

```text
offset:  0  1 2 3 4 5 6 7  8 9 10 11 12 13 14 15
         T  · · · · · · ·  [--------- age --------]
            seven padding bytes
```

Both examples normally have size 16 and alignment 8. Only the padding position changes. A declaration without `#[repr(C)]` uses Rust's default representation; its field layout is not an interface you should calculate or depend on because the compiler may choose a different order.

## Why `InlineFn` checks size and alignment

`InlineFn` stores a closure directly in a byte buffer when possible, avoiding a heap allocation. A closure fits inline only when both of these statements are true:

1. Its bytes fit within the buffer's capacity.
2. Its required alignment does not exceed the alignment guaranteed by the buffer.

The implementation expresses those independent requirements directly:

```rust
fn is_fit<T, const S: usize>() -> bool
{
    size_of::<T>() <= S && align_of::<T>() <= align_of::<FnBuffer<S>>()
}
```

A plain `[MaybeUninit<u8>; S]` has alignment 1. It may contain enough bytes for a captured pointer or integer, but its starting address is not guaranteed to be valid for that value. Writing a `T` through a misaligned `*mut T` would be undefined behavior.

`FnBuffer` raises the storage alignment explicitly:

```rust
#[repr(C, align(16))]
struct FnBuffer<const SIZE: usize>
{
    buffer: [MaybeUninit<u8>; SIZE],
}
```

An alignment of 16 can hold types requiring alignment 1, 2, 4, 8, or 16, provided their size also fits. A type requiring a larger alignment falls back to boxed storage, where the allocator supplies a suitable address.

Raising alignment can also raise size. Because a Rust type's size must preserve alignment between array elements, `FnBuffer<20>` occupies 32 bytes after rounding 20 up to the next multiple of 16. `FnBuffer<32>` and `FnBuffer<64>` need no extra padding.

## Rules of thumb

- Use `size_of::<T>()` to determine how many bytes Rust reserves for a `T`.
- Use `align_of::<T>()` when allocating storage or converting raw addresses into typed pointers.
- Advance a `*const T` or `*mut T` with `.add(n)`; Rust scales the offset by `size_of::<T>()`.
- Do not calculate Rust's default struct layout from its source field order.
- When deciding whether a value fits in raw storage, check both size and alignment.

## Further reading

- [Size, Stride, Alignment](https://swiftunboxed.com/internals/size-stride-alignment/), the Swift Unboxed article that inspired this explanation
- [Type layout](https://doc.rust-lang.org/reference/type-layout.html) in the Rust Reference
- [`std::mem::size_of`](https://doc.rust-lang.org/std/mem/fn.size_of.html)
- [`std::mem::align_of`](https://doc.rust-lang.org/std/mem/fn.align_of.html)
- [`std::ptr::write`](https://doc.rust-lang.org/std/ptr/fn.write.html)
