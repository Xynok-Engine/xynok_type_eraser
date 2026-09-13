//! [`InlineFn`], the reusable half: a type-erased `FnMut()`.

use std::marker::PhantomData;

use super::buffer::{FnBuffer, SMALL, assert_boxing_allowed, assert_buffer_size, is_fit};

/// Anything [`InlineFn`] can store: a callable that takes no arguments, returns
/// nothing, can be called more than once, and is safe to move across threads.
///
/// `FnMut` rather than `Fn`, so closures that mutate what they captured are
/// welcome. Every `Fn` closure satisfies this too, since `Fn: FnMut`. Closures
/// that consume what they captured only satisfy `FnOnce` and belong in
/// [`InlineFnOnce`](super::InlineFnOnce) instead.
pub trait IRunnable: FnMut() + Send {}
impl<T: FnMut() + Send> IRunnable for T {}

/// A type-erased `FnMut()` that stores its closure inline instead of on the heap.
///
/// A `Box<dyn FnMut()>` always allocates. `InlineFn` keeps a fixed byte buffer
/// in place and writes the closure straight into it, so the common case of a
/// small callback costs nothing beyond the buffer you already paid for.
/// Closures that do not fit fall back to a heap allocation, which keeps the type
/// usable everywhere rather than only for the sizes you guessed right.
///
/// # Parameters
///
/// - `S`: size of the inline buffer, in bytes. A closure fits when both its size
///   and its alignment fit. Defaults to [`SMALL`](super::SMALL).
/// - `D`: what to do when the closure does not fit. `true` boxes it silently,
///   `false` panics at construction. Use `false` when you need a hard guarantee
///   that no allocation happens, for example on an audio or render thread.
///
/// # Calling
///
/// [`run`](Self::run) takes `&mut self`, which is what lets the closure touch
/// its own state, and it can be called as many times as you like. That also
/// means an `InlineFn` behind a bare `Arc` cannot be called; wrap it in a
/// `Mutex` if several owners need to share one.
///
/// # Examples
///
/// ```
/// use xynok_type_eraser::inline_fn::{InlineFn, SMALL};
///
/// let mut hits = 0;
/// let mut task = InlineFn::<SMALL>::new(move || {
///     hits += 1;
///     println!("called {hits} times");
/// });
///
/// task.run();
/// task.run();
/// ```
///
/// Refusing to allocate, at the cost of a panic when the closure is too big:
///
/// ```should_panic
/// use xynok_type_eraser::inline_fn::InlineFn;
///
/// let payload = [0u8; 128];
/// let task = InlineFn::<32, false>::new(move || {
///     let _ = &payload;
/// });
/// ```
pub struct InlineFn<const S: usize = SMALL, const D: bool = true>
{
    buffer: FnBuffer<S>,
    vtable: &'static VTable<S>,
}

unsafe impl<const S: usize, const D: bool> Send for InlineFn<S, D> {}

impl<const S: usize, const D: bool> InlineFn<S, D>
{
    /// Stores `f`, inline when it fits and boxed when it does not.
    ///
    /// # Panics
    ///
    /// Panics when `S` is 0, and when `f` does not fit while `D` is `false`.
    #[track_caller]
    pub fn new<T: IRunnable>(f: T) -> Self
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
                    assert_boxing_allowed(D, std::any::type_name::<Self>(), S);
                    buffer.as_mut_ptr().cast::<Box<T>>().write(Box::new(f));
                    FnAlias::<T, S>::BOXED
                }
            }
        };

        Self { buffer, vtable }
    }

    /// Calls the stored closure. Borrows, so it can be called repeatedly.
    pub fn run(&mut self)
    {
        (self.vtable.runner)(&mut self.buffer)
    }

    /// Calls the stored closure once, then drops it.
    #[inline]
    pub fn run_once(mut self)
    {
        self.run();
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

impl<T: IRunnable, const S: usize> FnAlias<T, S>
{
    fn drop_inline(buffer: *mut FnBuffer<S>)
    {
        unsafe { buffer.cast::<T>().drop_in_place() };
    }

    fn run_inline(buffer: *mut FnBuffer<S>)
    {
        // `FnMut` is called through `&mut self`, so borrow instead of reading the
        // value out. Reading it out would drop the closure at the end of this
        // function and `Drop` would then drop it a second time.
        let f = unsafe { &mut *buffer.cast::<T>() };
        f();
    }
}

impl<T: IRunnable, const S: usize> FnAlias<T, S>
{
    fn drop_boxed(buffer: *mut FnBuffer<S>)
    {
        unsafe { buffer.cast::<Box<T>>().drop_in_place() };
    }

    fn run_boxed(buffer: *mut FnBuffer<S>)
    {
        // `&mut Box<T>` derefs to `&mut T`, so this is the inline case one hop away.
        let f = unsafe { &mut *buffer.cast::<Box<T>>() };
        f();
    }
}

impl<T: IRunnable, const S: usize> FnAlias<T, S>
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

impl<const S: usize, const D: bool> Drop for InlineFn<S, D>
{
    fn drop(&mut self)
    {
        (self.vtable.dropper)(&mut self.buffer);
    }
}

impl<const S: usize, const D: bool> std::fmt::Debug for InlineFn<S, D>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result
    {
        f.debug_struct(&format!("InlineFn<{S}>")).finish_non_exhaustive()
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
    fn run_once_calls_the_closure()
    {
        let flag = Arc::new(AtomicUsize::new(0));
        let f = {
            let flag = flag.clone();
            InlineFn::<SMALL>::new(move || {
                flag.fetch_add(1, Ordering::SeqCst);
            })
        };

        f.run_once();
        assert_eq!(flag.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_closure_can_mutate_what_it_captured()
    {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut count = 0;

        // Only `FnMut`, never `Fn`, since it writes back into `count`.
        let mut f = InlineFn::<SMALL>::new(move || {
            count += 1;
            tx.send(count).unwrap();
        });

        f.run();
        f.run();
        f.run();
        assert_eq!(rx.iter().take(3).collect::<Vec<_>>(), vec![1, 2, 3]);
    }

    #[test]
    fn captured_state_is_reachable_from_the_closure()
    {
        let (tx, rx) = std::sync::mpsc::channel();
        let f = InlineFn::<SMALL>::new(move || {
            tx.send(42u32).unwrap();
        });

        f.run_once();
        assert_eq!(rx.recv().unwrap(), 42);
    }

    #[test]
    fn oversized_closure_still_runs_through_the_boxed_path()
    {
        let payload = [7u8; LARGE * 2];
        let (tx, rx) = std::sync::mpsc::channel();

        // Bigger than SMALL, so this has to take the boxed path.
        let f = InlineFn::<SMALL, true>::new(move || {
            tx.send(payload.iter().map(|v| *v as usize).sum::<usize>()).unwrap();
        });

        f.run_once();
        assert_eq!(rx.recv().unwrap(), 7 * LARGE * 2);
    }

    #[test]
    fn a_bigger_buffer_holds_a_bigger_closure_inline()
    {
        let payload = [1u8; LARGE];
        let (tx, rx) = std::sync::mpsc::channel();

        let f = InlineFn::<{ LARGE * 4 }>::new(move || {
            tx.send(payload.len()).unwrap();
        });

        f.run_once();
        assert_eq!(rx.recv().unwrap(), LARGE);
    }

    #[test]
    fn run_can_be_called_repeatedly()
    {
        let counter = Arc::new(AtomicUsize::new(0));
        let mut f = {
            let counter = counter.clone();
            InlineFn::<SMALL>::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            })
        };

        f.run();
        f.run();
        f.run();
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn run_can_be_called_repeatedly_on_the_boxed_path()
    {
        let counter = Arc::new(AtomicUsize::new(0));
        let padding = [0u8; LARGE * 2];
        let mut f = {
            let counter = counter.clone();
            InlineFn::<SMALL, true>::new(move || {
                counter.fetch_add(padding.len(), Ordering::SeqCst);
            })
        };

        f.run();
        f.run();
        assert_eq!(counter.load(Ordering::SeqCst), LARGE * 4);
    }

    #[test]
    fn dropping_without_running_releases_captured_state()
    {
        let counter = Arc::new(AtomicUsize::new(0));
        let guard = DropCounter(counter.clone());

        let f = InlineFn::<SMALL>::new(move || {
            // Only here so `guard` is dropped together with the closure.
            let _ = &guard;
        });

        assert_eq!(counter.load(Ordering::SeqCst), 0);
        drop(f);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn run_once_releases_captured_state_exactly_once()
    {
        let counter = Arc::new(AtomicUsize::new(0));
        let guard = DropCounter(counter.clone());

        let f = InlineFn::<SMALL>::new(move || {
            let _ = &guard;
        });

        f.run_once();
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn repeated_runs_do_not_release_captured_state_early()
    {
        let counter = Arc::new(AtomicUsize::new(0));
        let guard = DropCounter(counter.clone());

        let mut f = InlineFn::<SMALL>::new(move || {
            let _ = &guard;
        });

        f.run();
        f.run();
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        drop(f);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn can_be_moved_to_another_thread()
    {
        let flag = Arc::new(AtomicUsize::new(0));
        let f = {
            let flag = flag.clone();
            InlineFn::<SMALL>::new(move || {
                flag.store(99, Ordering::SeqCst);
            })
        };

        std::thread::spawn(move || f.run_once()).join().unwrap();
        assert_eq!(flag.load(Ordering::SeqCst), 99);
    }

    #[test]
    fn debug_output_mentions_the_buffer_size()
    {
        let f = InlineFn::<SMALL>::new(|| {});
        let text = format!("{f:?}");
        assert!(text.contains(&format!("InlineFn<{SMALL}>")), "buffer size missing from: {text}");
    }

    #[test]
    #[should_panic(expected = "cannot fit")]
    fn oversized_closure_panics_when_boxing_is_disabled()
    {
        let payload = [0u8; LARGE * 2];
        let _ = InlineFn::<SMALL, false>::new(move || {
            let _ = &payload;
        });
    }

    #[test]
    #[should_panic(expected = "must be greater than 0")]
    fn zero_sized_buffer_panics()
    {
        let _ = InlineFn::<0>::new(|| {});
    }
}
