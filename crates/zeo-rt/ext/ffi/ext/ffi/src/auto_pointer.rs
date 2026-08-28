//! `FFI::AutoPointer < FFI::Pointer` -- a pointer that releases what it
//! points at when the object goes. The gem's own idiom for a C handle is to
//! SUBCLASS it and supply `self.release(ptr)`, which is why this file exists
//! at all: `AutoPointer` is a payload root (`zeo_abi::is_payload_root`), so
//! `class Handle < FFI::AutoPointer` compiles through the generic
//! `ValueSubclass` bridge with the pointer below as its payload. Every
//! read/write method comes from `FFI::Pointer` through the ancestry walk.
//!
//! Two documented divergences, both about WHEN the release runs:
//!  - zeo runs no finalizer, so a subclass's `self.release` is never called
//!    for it. Nothing leaks that Rust would not also have freed -- an owned
//!    buffer is reference counted -- but a handle whose release closes an OS
//!    resource stays open until the process exits.
//!  - CRuby raises `RuntimeError` from a BARE `FFI::AutoPointer.new(ptr)`
//!    ("no release method defined"); here that call answers a pointer. The
//!    check cannot be made where it belongs: a subclass reaches this same
//!    row through `construct_root_payload`, which passes the ROOT as the
//!    receiver, so the two are indistinguishable at this point.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::{RPointer, address_of};
use crate::RubyValue;
use zeo_macros::ruby_class;

ruby_class! {
    AutoPointer = zeo_abi::FFI_AUTO_POINTER_CLASS < zeo_abi::FFI_POINTER_CLASS;

    // `AutoPointer.new(ptr)` -- the address of whatever it is handed, with
    // `#autorelease?` true, which is the gem's default for this class (a raw
    // `Pointer` answers false; oracle-verified).
    def self."new" cfunc (_recv, source) {
        let addr = match address_of(source) {
            Some(a) => a,
            None => crate::ffi::to_i64(source)? as usize,
        };
        let p = RPointer::raw_at(addr, zeo_abi::FFI_AUTO_POINTER_CLASS);
        p.autorelease.store(true, Ordering::Relaxed);
        Ok(RubyValue::Object(Arc::new(p)))
    }
}
