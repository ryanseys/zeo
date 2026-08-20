//! The C-ABI surface Cranelift-compiled programs call -- `extern "C"`
//! twins of the internals the rustc backend reaches as ordinary Rust.
//! Every function here is a THIN wrapper: reuse, no reimplementation.
//!
//! # The boundary contract (every function's safety terms)
//!
//! * Ruby values cross as `*const`/`*mut RubyValue` pointing at live,
//!   properly aligned 24-byte slots the CALLER owns (`zeo_abi::abi`).
//!   `*const` arguments are borrowed; a `*mut` argument is MOVED out of
//!   (the caller must treat the slot as dead afterward); an `out` slot is
//!   written with `ptr::write` (the caller guarantees it holds no live
//!   value that would need dropping).
//! * `(ptr, len)` string pairs are UTF-8 bytes; the frame variants must be
//!   `'static` (`.rodata`) -- both are debug-asserted, never re-checked in
//!   release builds.
//! * Fallible functions return the `i32` status protocol: `STATUS_OK` with
//!   `out` written, or `STATUS_SIGNAL` with the `Signal` parked in the
//!   per-coroutine pending slot (`crate::signal`).
//! * Everything is single-coroutine state; callers never share a pointer
//!   across threads.
#![allow(
    clippy::missing_safety_doc,
    reason = "one boundary contract for the whole surface, documented on the module -- per-function repetition of the same pointer terms would drown the real signatures"
)]

pub mod bind;
pub mod dispatch;
pub mod ffi;
pub mod forloop;
pub mod frames;
pub mod kernel;
pub(crate) mod leakcheck;
pub mod lifecycle;
pub mod literals;
pub mod numeric;
pub mod objects;
pub mod patterns;
pub mod procs;
pub mod registry;
pub mod signals;
pub mod symbols;
pub mod values;

pub use procs::{BlockFn, ProcEnv};

/// A compiled method/trampoline body: the C twin of
/// [`crate::ValueMethodFn`], and the fn-pointer type dispatch rows and
/// inline caches store for Cranelift-emitted code ([`crate::ValueImpl`]'s
/// `C` arm). `recv`/`argv` are borrowed; `blk` is MOVED in (null = no
/// block; the callee consumes it); `out` receives the value on
/// `STATUS_OK`, and `STATUS_SIGNAL` parks the `Signal` in the pending
/// slot. `zeo-abi` mirrors this shape for the emitter once `ProgramDesc`
/// lands (M0-7); the two agree by the `abi_layout` assertions, not by
/// nominal identity.
pub type ValueFn = unsafe extern "C" fn(
    recv: *const crate::RubyValue,
    argv: *const crate::RubyValue,
    argc: usize,
    blk: *mut crate::RubyValue,
    out: *mut crate::RubyValue,
) -> i32;

/// A borrowed byte slice from a `(ptr, len)` pair. `len == 0` tolerates a
/// null `ptr` (an empty `Str` in a row table).
pub(crate) unsafe fn byte_slice<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if len == 0 {
        return &[];
    }
    unsafe { std::slice::from_raw_parts(ptr, len) }
}

/// A `(ptr, len)` pair as `&str`. UTF-8 is the emitter's guarantee,
/// debug-asserted here.
pub(crate) unsafe fn str_slice<'a>(ptr: *const u8, len: usize) -> &'a str {
    let bytes = unsafe { byte_slice(ptr, len) };
    debug_assert!(
        std::str::from_utf8(bytes).is_ok(),
        "non-UTF-8 bytes crossed the capi boundary"
    );
    unsafe { std::str::from_utf8_unchecked(bytes) }
}

/// A `(ptr, len)` pair as `&'static str` -- the frame-text variant, sound
/// only because the emitter passes `.rodata` strings (the contract above).
pub(crate) unsafe fn static_str(ptr: *const u8, len: usize) -> &'static str {
    unsafe { str_slice(ptr, len) }
}

#[cfg(test)]
mod tests;
