//! Storage shared by [`InlineFn`](super::InlineFn) and
//! [`InlineFnOnce`](super::InlineFnOnce).
//!
//! Both types differ only in how they call the closure they hold. Where they
//! put it, and how they decide whether it fits, is the same on both sides.

use std::mem::MaybeUninit;

/// `4 * size_of::<usize>()`. Fits a closure capturing up to four words.
pub const SMALL: usize = 32;

/// `8 * size_of::<usize>()`. Use it when the captured state is on the larger side.
pub const LARGE: usize = 64;

/// Raw storage for a closure.
///
/// The 16 byte alignment is what makes the inline path worth having. A plain
/// `[u8; S]` is only 1 byte aligned, which would make [`is_fit`] reject nearly
/// every closure that captures anything, since a captured pointer already needs
/// 8. See `docs/alignment.md` for the long version.
#[repr(C, align(16))]
pub(crate) struct FnBuffer<const SIZE: usize>
{
    buffer: [MaybeUninit<u8>; SIZE],
}

impl<const SIZE: usize> FnBuffer<SIZE>
{
    pub(crate) fn new() -> Self
    {
        Self {
            buffer: [MaybeUninit::uninit(); SIZE],
        }
    }

    pub(crate) fn as_mut_ptr(&mut self) -> *mut u8
    {
        self.buffer.as_mut_ptr().cast()
    }
}

/// Whether `T` can live inline in an `S` byte buffer.
///
/// Two separate questions. Size asks whether the closure fits at all, alignment
/// asks whether the buffer can put it at an address the closure is allowed to
/// sit at. Both have to hold, otherwise the closure goes on the heap where the
/// allocator handles alignment for us.
#[inline]
pub(crate) fn is_fit<T, const S: usize>() -> bool
{
    size_of::<T>() <= S && align_of::<T>() <= align_of::<FnBuffer<S>>()
}

/// A zero sized buffer would hand out a dangling pointer to write into.
#[track_caller]
pub(crate) fn assert_buffer_size<const S: usize>(type_name: &str)
{
    assert!(S > 0, "The `{type_name}` buffer size must be greater than 0");
}

/// Refuses the heap fallback when the caller asked for inline storage only.
///
/// The closure that did not fit has no name a user would recognise, so the
/// message spells out everything needed to fix it: the full type name of the
/// closure, what it needs, and what the buffer actually offers. Usually one of
/// the two numbers is the culprit, either the size or the alignment.
#[track_caller]
pub(crate) fn assert_boxing_allowed<T, const S: usize>(allowed: bool, type_name: &str)
{
    assert!(
        allowed,
        "`{type_name}` cannot store `{closure}` inline and boxing is off.\n\
         closure: size {closure_size} bytes, align {closure_align} bytes\n\
         buffer:  size {S} bytes, align {buffer_align} bytes\n\
         Raise the buffer size, or allow boxing, to store this closure.",
        closure = std::any::type_name::<T>(),
        closure_size = size_of::<T>(),
        closure_align = align_of::<T>(),
        buffer_align = align_of::<FnBuffer<S>>(),
    );
}

#[cfg(test)]
mod test
{
    use super::*;

    #[test]
    fn buffer_is_always_16_byte_aligned()
    {
        assert_eq!(align_of::<FnBuffer<SMALL>>(), 16);
        assert_eq!(align_of::<FnBuffer<1>>(), 16);
    }

    #[test]
    fn alignment_rounds_the_size_up()
    {
        assert_eq!(size_of::<FnBuffer<SMALL>>(), SMALL);
        assert_eq!(size_of::<FnBuffer<LARGE>>(), LARGE);
        assert_eq!(size_of::<FnBuffer<20>>(), 32);
    }

    #[test]
    fn is_fit_checks_both_size_and_alignment()
    {
        // Empty type: size 0, align 1, always fits.
        assert!(is_fit::<(), SMALL>());
        // A pointer needs align 8, well within what the buffer guarantees.
        assert!(is_fit::<Box<u8>, SMALL>());
        // u128 needs align 16, exactly what the buffer guarantees.
        assert!(is_fit::<u128, SMALL>());
        // Too big for the buffer, even though the alignment is fine.
        assert!(!is_fit::<[u8; LARGE], SMALL>());
    }
}
