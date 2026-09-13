//! [`InlineFnOnce`], the consuming half: a type-erased `FnOnce()`.

use std::marker::PhantomData;
use std::mem::ManuallyDrop;

use super::buffer::{FnBuffer, SMALL, assert_boxing_allowed, assert_buffer_size, is_fit};

/// Anything [`InlineFnOnce`] can store: a callable that takes no arguments,
/// returns nothing, and is safe to move across threads.
///
/// This is the widest of the three call traits. Since `Fn: FnMut: FnOnce`, every
/// closure accepted by [`InlineFn`](super::InlineFn) is accepted here too, plus
/// the ones that consume what they captured.
pub trait IRunnableOnce: FnOnce() + Send {}
impl<T: FnOnce() + Send> IRunnableOnce for T {}

/// A type-erased `FnOnce()` that stores its closure inline instead of on the heap.
///
/// Same storage strategy as [`InlineFn`](super::InlineFn), different contract:
/// the closure is consumed when it runs, so it runs at most once. That is what
/// buys you the wider set of accepted closures. Anything that moves its captured
/// state out, sends an owned value, or hands ownership to another function, fits
/// here and nowhere else.
///
/// Good fit for one-shot work: a task queued onto a thread pool, a completion
/// callback, a teardown hook.
///
/// # Parameters
///
/// - `S`: size of the inline buffer, in bytes. A closure fits when both its size
///   and its alignment fit. Defaults to [`SMALL`](super::SMALL).
/// - `D`: what to do when the closure does not fit. `true` boxes it silently,
///   `false` panics at construction.
///
/// # Calling
///
/// [`run`](Self::run) takes `self`. Calling it twice is not a runtime error, it
/// simply does not compile, since the value is gone after the first call. A
/// value that is never run drops its closure normally.
///
/// # Examples
///
/// ```
/// use xynok_type_eraser::inline_fn::{InlineFnOnce, SMALL};
///
/// let (tx, rx) = std::sync::mpsc::channel();
/// let name = String::from("worker");
///
/// // `name` is moved out of the closure, so this is `FnOnce` and nothing more.
/// let task = InlineFnOnce::<SMALL>::new(move || tx.send(name).unwrap());
///
/// task.run();
/// assert_eq!(rx.recv().unwrap(), "worker");
/// ```
pub struct InlineFnOnce<const S: usize = SMALL, const D: bool = true>
{
    buffer: FnBuffer<S>,
    vtable: &'static VTable<S>,
}

unsafe impl<const S: usize, const D: bool> Send for InlineFnOnce<S, D> {}

impl<const S: usize, const D: bool> InlineFnOnce<S, D>
{
    /// Stores `f`, inline when it fits and boxed when it does not.
    ///
    /// # Panics
    ///
    /// Panics when `S` is 0, and when `f` does not fit while `D` is `false`.
    #[track_caller]
    pub fn new<T: IRunnableOnce>(f: T) -> Self
    {
        assert_buffer_size::<S>(std::any::type_name::<Self>());

        let mut buffer = FnBuffer::<S>::new();
        let vtable = unsafe {
            match is_fit::<T, S>()
            {
                true =>
                {
                    buffer.as_mut_ptr().cast::<T>().write(f);
                    FnAlias::<T, S>::INLINE
                }
                false =>
                {
                    assert_boxing_allowed::<T, S>(D, std::any::type_name::<Self>());
                    buffer.as_mut_ptr().cast::<Box<T>>().write(Box::new(f));
                    FnAlias::<T, S>::BOXED
                }
            }
        };

        Self { buffer, vtable }
    }

    /// Calls the stored closure, consuming both it and `self`.
    pub fn run(self)
    {
        // The runner takes the closure out of the buffer and drops it as part of
        // calling it, so `Drop` must not run afterwards or it would drop twice.
        let mut this = ManuallyDrop::new(self);
        (this.vtable.runner)(&mut this.buffer);
    }
}

/// Hand written vtable, one shared static per `(closure type, S, storage)` combo.
struct VTable<const S: usize>
{
    dropper: fn(*mut FnBuffer<S>),
    runner:  fn(*mut FnBuffer<S>),
}

/// Carries `T` so the vtable functions can be monomorphised, without making the
/// struct own or require a `T`. `PhantomData<fn() -> T>` keeps it covariant and
/// keeps auto traits from leaking in.
///
/// See https://github.com/rust-lang/nomicon/issues/320
struct FnAlias<T, const S: usize>(PhantomData<fn() -> T>);

impl<T: IRunnableOnce, const S: usize> FnAlias<T, S>
{
    fn drop_inline(buffer: *mut FnBuffer<S>)
    {
        unsafe { buffer.cast::<T>().drop_in_place() };
    }

    fn run_inline(buffer: *mut FnBuffer<S>)
    {
        // `FnOnce` is called through `self`, so the closure has to come out of the
        // buffer by value. It is dropped at the end of this call, which is why
        // `run` wraps the whole thing in `ManuallyDrop`.
        let f = unsafe { buffer.cast::<T>().read() };
        f();
    }
}

impl<T: IRunnableOnce, const S: usize> FnAlias<T, S>
{
    fn drop_boxed(buffer: *mut FnBuffer<S>)
    {
        unsafe { buffer.cast::<Box<T>>().drop_in_place() };
    }

    fn run_boxed(buffer: *mut FnBuffer<S>)
    {
        // `Box<T>` is itself `FnOnce` when `T` is, so calling it consumes the box
        // and frees the allocation.
        let f = unsafe { buffer.cast::<Box<T>>().read() };
        f();
    }
}

impl<T: IRunnableOnce, const S: usize> FnAlias<T, S>
{
    const INLINE: &VTable<S> = &VTable::<S> {
        dropper: Self::drop_inline,
        runner:  Self::run_inline,
    };
    const BOXED: &VTable<S> = &VTable::<S> {
        dropper: Self::drop_boxed,
        runner:  Self::run_boxed,
    };
}

impl<const S: usize, const D: bool> Drop for InlineFnOnce<S, D>
{
    fn drop(&mut self)
    {
        (self.vtable.dropper)(&mut self.buffer);
    }
}

impl<const S: usize, const D: bool> std::fmt::Debug for InlineFnOnce<S, D>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result
    {
        f.debug_struct(&format!("InlineFnOnce<{S}>")).finish_non_exhaustive()
    }
}

#[cfg(test)]
mod test
{
    use super::super::buffer::LARGE;
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Counts its own drops, so tests can check when captured state is released.
    struct DropCounter(Arc<AtomicUsize>);

    impl Drop for DropCounter
    {
        fn drop(&mut self)
        {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn run_calls_the_closure()
    {
        let flag = Arc::new(AtomicUsize::new(0));
        let f = {
            let flag = flag.clone();
            InlineFnOnce::<SMALL>::new(move || {
                flag.fetch_add(1, Ordering::SeqCst);
            })
        };

        f.run();
        assert_eq!(flag.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_closure_can_consume_what_it_captured()
    {
        let (tx, rx) = std::sync::mpsc::channel();
        let owned = String::from("moved out");

        // Only `FnOnce`, never `FnMut`, since `owned` leaves the closure.
        let f = InlineFnOnce::<SMALL>::new(move || tx.send(owned).unwrap());

        f.run();
        assert_eq!(rx.recv().unwrap(), "moved out");
    }

    #[test]
    fn oversized_closure_still_runs_through_the_boxed_path()
    {
        let payload = [7u8; LARGE * 2];
        let (tx, rx) = std::sync::mpsc::channel();

        // Bigger than SMALL, so this has to take the boxed path.
        let f = InlineFnOnce::<SMALL, true>::new(move || {
            tx.send(payload.iter().map(|v| *v as usize).sum::<usize>()).unwrap();
        });

        f.run();
        assert_eq!(rx.recv().unwrap(), 7 * LARGE * 2);
    }

    #[test]
    fn a_bigger_buffer_holds_a_bigger_closure_inline()
    {
        let payload = [1u8; LARGE];
        let (tx, rx) = std::sync::mpsc::channel();

        let f = InlineFnOnce::<{ LARGE * 4 }>::new(move || {
            tx.send(payload.len()).unwrap();
        });

        f.run();
        assert_eq!(rx.recv().unwrap(), LARGE);
    }

    #[test]
    fn dropping_without_running_releases_captured_state()
    {
        let counter = Arc::new(AtomicUsize::new(0));
        let guard = DropCounter(counter.clone());

        let f = InlineFnOnce::<SMALL>::new(move || {
            // Only here so `guard` is dropped together with the closure.
            let _ = &guard;
        });

        assert_eq!(counter.load(Ordering::SeqCst), 0);
        drop(f);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn run_releases_captured_state_exactly_once()
    {
        let counter = Arc::new(AtomicUsize::new(0));
        let guard = DropCounter(counter.clone());

        let f = InlineFnOnce::<SMALL>::new(move || {
            let _ = &guard;
        });

        f.run();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn run_releases_captured_state_exactly_once_on_the_boxed_path()
    {
        let counter = Arc::new(AtomicUsize::new(0));
        let guard = DropCounter(counter.clone());
        let padding = [0u8; LARGE * 2];

        let f = InlineFnOnce::<SMALL, true>::new(move || {
            let _ = &guard;
            let _ = &padding;
        });

        f.run();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn can_be_moved_to_another_thread()
    {
        let flag = Arc::new(AtomicUsize::new(0));
        let f = {
            let flag = flag.clone();
            InlineFnOnce::<SMALL>::new(move || {
                flag.store(99, Ordering::SeqCst);
            })
        };

        std::thread::spawn(move || f.run()).join().unwrap();
        assert_eq!(flag.load(Ordering::SeqCst), 99);
    }

    #[test]
    fn debug_output_mentions_the_buffer_size()
    {
        let f = InlineFnOnce::<SMALL>::new(|| {});
        let text = format!("{f:?}");
        assert!(text.contains(&format!("InlineFnOnce<{SMALL}>")), "buffer size missing from: {text}");
    }

    #[test]
    #[should_panic(expected = "cannot store")]
    fn oversized_closure_panics_when_boxing_is_disabled()
    {
        let payload = [0u8; LARGE * 2];
        let _ = InlineFnOnce::<SMALL, false>::new(move || {
            let _ = &payload;
        });
    }

    #[test]
    #[should_panic(expected = "must be greater than 0")]
    fn zero_sized_buffer_panics()
    {
        let _ = InlineFnOnce::<0>::new(|| {});
    }
}
