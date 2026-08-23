//! The C extension surface.
//!
//! zeo compiles a gem's `ext/**/*.c` from source against MRI's own headers
//! (`crates/zeo-rt/cext/`, whose README states the stance) and never loads a
//! prebuilt MRI `.so`. This module is the other side of that boundary: the
//! `VALUE` encoding, the handles a heap `VALUE` points at, and the scope that
//! decides how long one lives.
//!
//! It is the one place the runtime's `unsafe` discipline changes. Everywhere
//! else, `unsafe` is a local claim about a pointer this crate made. Here, a C
//! function the project has never seen is handed a pointer and trusted with
//! it -- so the rules that matter are the ones the extension cannot break by
//! accident: the encoding is MRI's bit for bit, a handle is canonical per
//! object, and a layout reader that zeo cannot answer raises rather than
//! guesses.

pub mod value;
