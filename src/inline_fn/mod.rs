//! Type-erased closures that avoid the heap when they can.
//!
//! Two types, same storage, different call contract. Pick by how the closure
//! treats the state it captured:
//!
//! | Closure does | Implements | Use |
//! | --- | --- | --- |
//! | only reads captured state | `Fn` | [`InlineFn`] |
//! | writes back into captured state | `FnMut` | [`InlineFn`] |
//! | moves captured state out | `FnOnce` | [`InlineFnOnce`] |
//!
//! The traits nest, `Fn: FnMut: FnOnce`, so [`InlineFnOnce`] accepts everything
//! [`InlineFn`] does and more. The catch is that it can only run once, since
//! `FnOnce` consumes the closure when called. [`InlineFn`] gives up those extra
//! closures and gets a `run` you can call as often as you like in return.
//!
//! Both store the closure inline in a fixed buffer, falling back to a heap
//! allocation when it does not fit. See [`buffer`] for the shared storage and
//! `docs/alignment.md` for why the buffer is aligned the way it is.

mod buffer;
mod fn_mut;
mod fn_once;

pub use buffer::{LARGE, SMALL};
pub use fn_mut::{IRunnable, InlineFn};
pub use fn_once::{IRunnableOnce, InlineFnOnce};
