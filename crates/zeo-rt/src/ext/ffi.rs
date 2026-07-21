//! The `ffi` gem's memory classes (#204 follow-ons): `FFI::Pointer` and
//! `FFI::MemoryPointer`. Unlike the `attach_function` machinery (which is a
//! pure compile-time `extern "C"` emission -- see `zeo`'s `emit_ffi_call`
//! and this crate's `crate::ffi` marshaling), these are real runtime values
//! that flow through Ruby code: a `MemoryPointer` allocates a heap buffer and
//! answers `read_int`/`write_int`/`[]`/`+`, and a `:pointer` return from a C
//! function is wrapped as an `FFI::Pointer`. The surface is oracle-verified
//! against `ffi 1.17.4`.
//!
//! Memory model: an `RPointer` is a raw C address (`base`) plus an optional
//! owned heap buffer (`owner`) that keeps that address alive. A `MemoryPointer`
//! owns its buffer; a `Pointer` wrapping a raw address (`Pointer.new(addr)`, or
//! a C return) does not. Pointer arithmetic (`ptr + n`) shares the owner and
//! advances the address, exactly like the gem. Reads/writes are raw unaligned
//! accesses, so the same code serves an owned Ruby buffer and foreign C memory.
//!
//! Documented divergences from CRuby+ffi: (1) an unsigned 64-bit read whose top
//! bit is set wraps to a negative `Integer` -- this runtime's `Integer` is
//! `i64`, with no Bignum tier (the same limit the scalar `attach_function`
//! marshaling has). (2) `#address`/`#inspect` expose a real heap address for an
//! owned buffer, so they are non-deterministic and never golden-tested.

use crate::builtins::{arg_error, arity, builtin_methods, index_error, type_error};
use crate::dispatch::{RObj, RubyObject};
use crate::{ClassId, RubyValue, Signal, Symbol};
use std::alloc::{Layout, alloc_zeroed, dealloc};
use std::os::raw::c_char;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use zeo_abi::{FFI_MEMORY_POINTER_CLASS, FFI_POINTER_CLASS};

/// A heap buffer an owned pointer allocated, freed when the last pointer
/// sharing it (via pointer arithmetic) drops. Raw alloc rather than `Vec` so
/// the data address is stable and real (what `#address` and a `:pointer`
/// argument to C both hand out).
struct OwnedBuf {
    ptr: *mut u8,
    layout: Layout,
}

impl OwnedBuf {
    fn zeroed(size: usize) -> Arc<OwnedBuf> {
        // A zero-byte allocation is UB; the gem still lets you make a 0-length
        // pointer, so round the allocation up to 1 while reporting size 0.
        let layout = Layout::from_size_align(size.max(1), 8).expect("valid FFI layout");
        let ptr = unsafe { alloc_zeroed(layout) };
        assert!(!ptr.is_null(), "FFI memory allocation failed");
        Arc::new(OwnedBuf { ptr, layout })
    }
}

impl Drop for OwnedBuf {
    fn drop(&mut self) {
        unsafe { dealloc(self.ptr, self.layout) }
    }
}

// A raw C buffer is inherently thread-shareable; the `ffi` gem places the same
// (un)safety contract on its pointers.
unsafe impl Send for OwnedBuf {}
unsafe impl Sync for OwnedBuf {}

/// An `FFI::Pointer` / `FFI::MemoryPointer` instance.
pub struct RPointer {
    /// The real C address this points at (owned buffer data, or a raw address).
    base: *mut u8,
    /// Byte size when known (owned buffers); `None` for a raw address whose
    /// extent the gem also doesn't know.
    size: Option<usize>,
    /// The owned buffer keeping `base` alive; `None` for a raw address.
    owner: Option<Arc<OwnedBuf>>,
    /// `FFI_POINTER_CLASS` or `FFI_MEMORY_POINTER_CLASS` -- governs `.class`.
    class: ClassId,
    frozen: AtomicBool,
}

unsafe impl Send for RPointer {}
unsafe impl Sync for RPointer {}

impl RPointer {
    /// A `MemoryPointer`-style owned, zeroed buffer of `size` bytes.
    fn owned(size: usize, class: ClassId) -> RPointer {
        let owner = OwnedBuf::zeroed(size);
        RPointer {
            base: owner.ptr,
            size: Some(size),
            owner: Some(owner),
            class,
            frozen: AtomicBool::new(false),
        }
    }

    /// An owned buffer holding `bytes` followed by a NUL (`from_string`).
    fn from_bytes_nul(bytes: &[u8]) -> RPointer {
        let p = RPointer::owned(bytes.len() + 1, FFI_MEMORY_POINTER_CLASS);
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), p.base, bytes.len()) };
        p
    }

    /// A `Pointer` over a raw address it does not own.
    fn raw(addr: usize, class: ClassId) -> RPointer {
        RPointer {
            base: addr as *mut u8,
            size: None,
            owner: None,
            class,
            frozen: AtomicBool::new(false),
        }
    }

    /// `ptr + delta` -- a fresh `Pointer` sharing the owner, advanced by
    /// `delta` bytes (the size shrinks to match, as the gem's does).
    fn offset(&self, delta: usize) -> RPointer {
        RPointer {
            base: unsafe { self.base.add(delta) },
            size: self.size.map(|s| s.saturating_sub(delta)),
            owner: self.owner.clone(),
            class: FFI_POINTER_CLASS,
            frozen: AtomicBool::new(false),
        }
    }

    fn address(&self) -> usize {
        self.base as usize
    }

    /// Reject an out-of-bounds access on a sized (owned) buffer, exactly where
    /// the gem raises `IndexError`; a raw address (unknown size) is unchecked.
    fn check_bounds(&self, off: usize, len: usize) -> Result<(), Signal> {
        if let Some(size) = self.size {
            if off + len > size {
                return Err(index_error!(
                    "Memory access offset={off} size={len} out of bounds (total {size})"
                ));
            }
        }
        Ok(())
    }

    unsafe fn read_int(&self, off: usize, bytes: usize, signed: bool) -> i64 {
        unsafe {
            let p = self.base.add(off);
            match (bytes, signed) {
                (1, true) => (p as *const i8).read_unaligned() as i64,
                (1, false) => (p as *const u8).read_unaligned() as i64,
                (2, true) => (p as *const i16).read_unaligned() as i64,
                (2, false) => (p as *const u16).read_unaligned() as i64,
                (4, true) => (p as *const i32).read_unaligned() as i64,
                (4, false) => (p as *const u32).read_unaligned() as i64,
                (8, true) => (p as *const i64).read_unaligned(),
                (8, false) => (p as *const u64).read_unaligned() as i64,
                _ => unreachable!("FFI int width is 1/2/4/8"),
            }
        }
    }

    unsafe fn write_int(&self, off: usize, bytes: usize, v: i64) {
        unsafe {
            let p = self.base.add(off);
            match bytes {
                1 => p.write_unaligned(v as u8),
                2 => (p as *mut u16).write_unaligned(v as u16),
                4 => (p as *mut u32).write_unaligned(v as u32),
                8 => (p as *mut u64).write_unaligned(v as u64),
                _ => unreachable!("FFI int width is 1/2/4/8"),
            }
        }
    }

    unsafe fn read_float(&self, off: usize, bytes: usize) -> f64 {
        unsafe {
            let p = self.base.add(off);
            match bytes {
                4 => (p as *const f32).read_unaligned() as f64,
                8 => (p as *const f64).read_unaligned(),
                _ => unreachable!("FFI float width is 4/8"),
            }
        }
    }

    unsafe fn write_float(&self, off: usize, bytes: usize, v: f64) {
        unsafe {
            let p = self.base.add(off);
            match bytes {
                4 => (p as *mut f32).write_unaligned(v as f32),
                8 => (p as *mut f64).write_unaligned(v),
                _ => unreachable!("FFI float width is 4/8"),
            }
        }
    }
}

impl RubyObject for RPointer {
    fn class_id(&self) -> ClassId {
        self.class
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        self.frozen.load(Ordering::Relaxed)
    }
    fn set_frozen(&self) {
        self.frozen.store(true, Ordering::Relaxed)
    }
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        // A dup is a second handle onto the same memory (a shallow copy, as the
        // gem's `Pointer#dup` is), sharing the owner.
        let p = RPointer {
            base: self.base,
            size: self.size,
            owner: self.owner.clone(),
            class: self.class,
            frozen: AtomicBool::new(false),
        };
        if copy_frozen {
            p.set_frozen();
        }
        Arc::new(p)
    }
}

/// The `RPointer` behind a receiver -- the table only dispatches on one.
fn ptr_of(recv: &RubyValue) -> &RPointer {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RPointer>()
            .expect("the FFI pointer table only dispatches on pointer receivers"),
        _ => unreachable!("the FFI pointer table only dispatches on pointer receivers"),
    }
}

// ---- constructors + accessors used by the `crate::ffi` marshaling boundary ----

/// A fresh zeroed `MemoryPointer` of `size` bytes (used by `MemoryPointer.new`
/// and by any C call that needs a scratch buffer).
pub fn new_memory(size: usize) -> RubyValue {
    RubyValue::Object(Arc::new(RPointer::owned(size, FFI_MEMORY_POINTER_CLASS)))
}

/// Wrap a raw C address as an `FFI::Pointer` (a `:pointer` return value).
pub fn wrap_address(addr: usize) -> RubyValue {
    RubyValue::Object(Arc::new(RPointer::raw(addr, FFI_POINTER_CLASS)))
}

/// The raw C address a pointer-typed argument carries, or `None` if `v` is not
/// a pointer (`nil` is a NULL pointer -- the gem accepts it for `:pointer`).
pub fn address_of(v: &RubyValue) -> Option<usize> {
    match v {
        RubyValue::Nil => Some(0),
        RubyValue::Object(o) => o.as_any().downcast_ref::<RPointer>().map(|p| p.address()),
        _ => None,
    }
}

// ---- argument helpers ----

/// An integer argument at `idx`, defaulting to 0 (an absent offset).
fn off_arg(args: &[RubyValue], idx: usize) -> Result<usize, Signal> {
    match args.get(idx) {
        None => Ok(0),
        Some(v) => Ok(crate::ffi::to_i64(v)? as usize),
    }
}

/// `read_*` (offset 0) / `get_*(offset)` share this: `with_off` picks whether
/// the offset comes from an argument or is 0.
fn read_int_m(
    recv: &RubyValue,
    args: &[RubyValue],
    bytes: usize,
    signed: bool,
    with_off: bool,
) -> Result<RubyValue, Signal> {
    let off = if with_off {
        arity!(args, 1);
        crate::ffi::to_i64(&args[0])? as usize
    } else {
        arity!(args, 0);
        0
    };
    let p = ptr_of(recv);
    p.check_bounds(off, bytes)?;
    Ok(RubyValue::Int(unsafe { p.read_int(off, bytes, signed) }))
}

/// `write_*(value)` (offset 0) / `put_*(offset, value)`.
fn write_int_m(
    recv: &RubyValue,
    args: &[RubyValue],
    bytes: usize,
    with_off: bool,
) -> Result<RubyValue, Signal> {
    let (off, v) = if with_off {
        arity!(args, 2);
        (
            crate::ffi::to_i64(&args[0])? as usize,
            crate::ffi::to_i64(&args[1])?,
        )
    } else {
        arity!(args, 1);
        (0, crate::ffi::to_i64(&args[0])?)
    };
    let p = ptr_of(recv);
    p.check_bounds(off, bytes)?;
    unsafe { p.write_int(off, bytes, v) };
    Ok(recv.clone())
}

fn read_float_m(
    recv: &RubyValue,
    args: &[RubyValue],
    bytes: usize,
    with_off: bool,
) -> Result<RubyValue, Signal> {
    let off = if with_off {
        arity!(args, 1);
        crate::ffi::to_i64(&args[0])? as usize
    } else {
        arity!(args, 0);
        0
    };
    let p = ptr_of(recv);
    p.check_bounds(off, bytes)?;
    Ok(RubyValue::Float(unsafe { p.read_float(off, bytes) }))
}

fn write_float_m(
    recv: &RubyValue,
    args: &[RubyValue],
    bytes: usize,
    with_off: bool,
) -> Result<RubyValue, Signal> {
    let (off, v) = if with_off {
        arity!(args, 2);
        (
            crate::ffi::to_i64(&args[0])? as usize,
            crate::ffi::to_f64(&args[1])?,
        )
    } else {
        arity!(args, 1);
        (0, crate::ffi::to_f64(&args[0])?)
    };
    let p = ptr_of(recv);
    p.check_bounds(off, bytes)?;
    unsafe { p.write_float(off, bytes, v) };
    Ok(recv.clone())
}

/// A String's bytes, for a `put_string`/`write_bytes` argument.
fn str_bytes(v: &RubyValue) -> Result<Vec<u8>, Signal> {
    Ok(crate::builtins::convert::to_rstr(v)?
        .lock()
        .bytes()
        .to_vec())
}

fn bytes_to_str(bytes: Vec<u8>) -> RubyValue {
    RubyValue::Str(crate::string_from_bytes(
        bytes,
        crate::encoding::default_external(),
    ))
}

builtin_methods! {
    pub(crate) fn lookup;

    // -- signed/unsigned integer reads (offset 0) --
    "read_int8" | "read_char" => fn read_int8(r, a, _b) { read_int_m(r, a, 1, true, false) }
    "read_uint8" | "read_uchar" => fn read_uint8(r, a, _b) { read_int_m(r, a, 1, false, false) }
    "read_int16" | "read_short" => fn read_int16(r, a, _b) { read_int_m(r, a, 2, true, false) }
    "read_uint16" | "read_ushort" => fn read_uint16(r, a, _b) { read_int_m(r, a, 2, false, false) }
    "read_int32" | "read_int" => fn read_int32(r, a, _b) { read_int_m(r, a, 4, true, false) }
    "read_uint32" | "read_uint" => fn read_uint32(r, a, _b) { read_int_m(r, a, 4, false, false) }
    "read_int64" | "read_long" | "read_long_long" => fn read_int64(r, a, _b) { read_int_m(r, a, 8, true, false) }
    "read_uint64" | "read_ulong" | "read_ulong_long" => fn read_uint64(r, a, _b) { read_int_m(r, a, 8, false, false) }

    // -- integer reads at an offset --
    "get_int8" | "get_char" => fn get_int8(r, a, _b) { read_int_m(r, a, 1, true, true) }
    "get_uint8" | "get_uchar" => fn get_uint8(r, a, _b) { read_int_m(r, a, 1, false, true) }
    "get_int16" | "get_short" => fn get_int16(r, a, _b) { read_int_m(r, a, 2, true, true) }
    "get_uint16" | "get_ushort" => fn get_uint16(r, a, _b) { read_int_m(r, a, 2, false, true) }
    "get_int32" | "get_int" => fn get_int32(r, a, _b) { read_int_m(r, a, 4, true, true) }
    "get_uint32" | "get_uint" => fn get_uint32(r, a, _b) { read_int_m(r, a, 4, false, true) }
    "get_int64" | "get_long" | "get_long_long" => fn get_int64(r, a, _b) { read_int_m(r, a, 8, true, true) }
    "get_uint64" | "get_ulong" | "get_ulong_long" => fn get_uint64(r, a, _b) { read_int_m(r, a, 8, false, true) }

    // -- integer writes (offset 0) --
    "write_int8" | "write_char" => fn write_int8(r, a, _b) { write_int_m(r, a, 1, false) }
    "write_uint8" | "write_uchar" => fn write_uint8(r, a, _b) { write_int_m(r, a, 1, false) }
    "write_int16" | "write_short" => fn write_int16(r, a, _b) { write_int_m(r, a, 2, false) }
    "write_uint16" | "write_ushort" => fn write_uint16(r, a, _b) { write_int_m(r, a, 2, false) }
    "write_int32" | "write_int" => fn write_int32(r, a, _b) { write_int_m(r, a, 4, false) }
    "write_uint32" | "write_uint" => fn write_uint32(r, a, _b) { write_int_m(r, a, 4, false) }
    "write_int64" | "write_long" | "write_long_long" => fn write_int64(r, a, _b) { write_int_m(r, a, 8, false) }
    "write_uint64" | "write_ulong" | "write_ulong_long" => fn write_uint64(r, a, _b) { write_int_m(r, a, 8, false) }

    // -- integer writes at an offset --
    "put_int8" | "put_char" => fn put_int8(r, a, _b) { write_int_m(r, a, 1, true) }
    "put_uint8" | "put_uchar" => fn put_uint8(r, a, _b) { write_int_m(r, a, 1, true) }
    "put_int16" | "put_short" => fn put_int16(r, a, _b) { write_int_m(r, a, 2, true) }
    "put_uint16" | "put_ushort" => fn put_uint16(r, a, _b) { write_int_m(r, a, 2, true) }
    "put_int32" | "put_int" => fn put_int32(r, a, _b) { write_int_m(r, a, 4, true) }
    "put_uint32" | "put_uint" => fn put_uint32(r, a, _b) { write_int_m(r, a, 4, true) }
    "put_int64" | "put_long" | "put_long_long" => fn put_int64(r, a, _b) { write_int_m(r, a, 8, true) }
    "put_uint64" | "put_ulong" | "put_ulong_long" => fn put_uint64(r, a, _b) { write_int_m(r, a, 8, true) }

    // -- floats --
    "read_float" => fn read_float32(r, a, _b) { read_float_m(r, a, 4, false) }
    "read_double" => fn read_float64(r, a, _b) { read_float_m(r, a, 8, false) }
    "get_float32" | "get_float" => fn get_float32(r, a, _b) { read_float_m(r, a, 4, true) }
    "get_float64" | "get_double" => fn get_float64(r, a, _b) { read_float_m(r, a, 8, true) }
    "write_float" => fn write_float32(r, a, _b) { write_float_m(r, a, 4, false) }
    "write_double" => fn write_float64(r, a, _b) { write_float_m(r, a, 8, false) }
    "put_float32" | "put_float" => fn put_float32(r, a, _b) { write_float_m(r, a, 4, true) }
    "put_float64" | "put_double" => fn put_float64(r, a, _b) { write_float_m(r, a, 8, true) }

    // -- pointers (read/write an address-sized word, wrapped as a Pointer) --
    "read_pointer" | "get_pointer" => fn read_pointer(recv, args, _b) {
        arity!(args, 0..=1);
        let off = off_arg(args, 0)?;
        let p = ptr_of(recv);
        p.check_bounds(off, 8)?;
        Ok(wrap_address(unsafe { p.read_int(off, 8, false) } as usize))
    }
    "write_pointer" | "put_pointer" => fn write_pointer(recv, args, _b) {
        arity!(args, 1..=2);
        let (off, target) = if args.len() == 2 { (off_arg(args, 0)?, &args[1]) } else { (0, &args[0]) };
        let addr = address_of(target).ok_or_else(|| type_error!("wrong argument type (expected a pointer)"))?;
        let p = ptr_of(recv);
        p.check_bounds(off, 8)?;
        unsafe { p.write_int(off, 8, addr as i64) };
        Ok(recv.clone())
    }

    // -- strings & raw bytes --
    // `read_string` -> up to the first NUL; `read_string(len)` -> exactly len bytes.
    "read_string" => fn read_string(recv, args, _b) {
        arity!(args, 0..=1);
        let p = ptr_of(recv);
        match args.first() {
            None | Some(RubyValue::Nil) => {
                let bytes = unsafe { std::ffi::CStr::from_ptr(p.base as *const c_char) }.to_bytes().to_vec();
                Ok(bytes_to_str(bytes))
            }
            Some(len) => {
                let n = crate::ffi::to_i64(len)? as usize;
                p.check_bounds(0, n)?;
                let bytes = unsafe { std::slice::from_raw_parts(p.base, n) }.to_vec();
                Ok(bytes_to_str(bytes))
            }
        }
    }
    // `get_string(offset, length = nil)` -- NUL-terminated at offset, or fixed length.
    "get_string" => fn get_string(recv, args, _b) {
        arity!(args, 1..=2);
        let off = crate::ffi::to_i64(&args[0])? as usize;
        let p = ptr_of(recv);
        match args.get(1) {
            None | Some(RubyValue::Nil) => {
                let bytes = unsafe { std::ffi::CStr::from_ptr(p.base.add(off) as *const c_char) }.to_bytes().to_vec();
                Ok(bytes_to_str(bytes))
            }
            Some(len) => {
                let n = crate::ffi::to_i64(len)? as usize;
                p.check_bounds(off, n)?;
                let bytes = unsafe { std::slice::from_raw_parts(p.base.add(off), n) }.to_vec();
                Ok(bytes_to_str(bytes))
            }
        }
    }
    // `put_string(offset, str)` writes the bytes plus a terminating NUL.
    "put_string" => fn put_string(recv, args, _b) {
        arity!(args, 2);
        let off = crate::ffi::to_i64(&args[0])? as usize;
        let bytes = str_bytes(&args[1])?;
        let p = ptr_of(recv);
        p.check_bounds(off, bytes.len() + 1)?;
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), p.base.add(off), bytes.len());
            p.base.add(off + bytes.len()).write(0);
        }
        Ok(recv.clone())
    }
    // `read_bytes(len)` / `get_bytes(offset, len)` -- raw bytes, NUL-agnostic.
    "read_bytes" => fn read_bytes(recv, args, _b) {
        arity!(args, 1);
        let n = crate::ffi::to_i64(&args[0])? as usize;
        let p = ptr_of(recv);
        p.check_bounds(0, n)?;
        Ok(bytes_to_str(unsafe { std::slice::from_raw_parts(p.base, n) }.to_vec()))
    }
    "get_bytes" => fn get_bytes(recv, args, _b) {
        arity!(args, 2);
        let off = crate::ffi::to_i64(&args[0])? as usize;
        let n = crate::ffi::to_i64(&args[1])? as usize;
        let p = ptr_of(recv);
        p.check_bounds(off, n)?;
        Ok(bytes_to_str(unsafe { std::slice::from_raw_parts(p.base.add(off), n) }.to_vec()))
    }
    // `put_bytes(offset, str)` / `write_bytes(str)` -- raw bytes, no NUL.
    "put_bytes" => fn put_bytes(recv, args, _b) {
        arity!(args, 2);
        let off = crate::ffi::to_i64(&args[0])? as usize;
        let bytes = str_bytes(&args[1])?;
        let p = ptr_of(recv);
        p.check_bounds(off, bytes.len())?;
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), p.base.add(off), bytes.len()) };
        Ok(recv.clone())
    }
    "write_bytes" => fn write_bytes(recv, args, _b) {
        arity!(args, 1);
        let bytes = str_bytes(&args[0])?;
        let p = ptr_of(recv);
        p.check_bounds(0, bytes.len())?;
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), p.base, bytes.len()) };
        Ok(recv.clone())
    }

    // -- typed arrays --
    "read_array_of_int8" => fn raoi8(r, a, _b) { read_int_array(r, a, 1, true) }
    "read_array_of_uint8" => fn raou8(r, a, _b) { read_int_array(r, a, 1, false) }
    "read_array_of_int16" => fn raoi16(r, a, _b) { read_int_array(r, a, 2, true) }
    "read_array_of_int32" | "read_array_of_int" => fn raoi32(r, a, _b) { read_int_array(r, a, 4, true) }
    "read_array_of_uint32" | "read_array_of_uint" => fn raou32(r, a, _b) { read_int_array(r, a, 4, false) }
    "read_array_of_int64" | "read_array_of_long" => fn raoi64(r, a, _b) { read_int_array(r, a, 8, true) }
    "write_array_of_int8" => fn waoi8(r, a, _b) { write_int_array(r, a, 1) }
    "write_array_of_int16" => fn waoi16(r, a, _b) { write_int_array(r, a, 2) }
    "write_array_of_int32" | "write_array_of_int" => fn waoi32(r, a, _b) { write_int_array(r, a, 4) }
    "write_array_of_int64" | "write_array_of_long" => fn waoi64(r, a, _b) { write_int_array(r, a, 8) }
    "read_array_of_double" => fn raod(r, a, _b) { read_float_array(r, a, 8) }
    "read_array_of_float" => fn raof(r, a, _b) { read_float_array(r, a, 4) }
    "write_array_of_double" => fn waod(r, a, _b) { write_float_array(r, a, 8) }
    "write_array_of_float" => fn waof(r, a, _b) { write_float_array(r, a, 4) }

    // -- identity / arithmetic --
    "null?" => fn is_null(recv, args, _b) {
        arity!(args, 0);
        Ok(RubyValue::Bool(ptr_of(recv).address() == 0))
    }
    "address" | "to_i" => fn address(recv, args, _b) {
        arity!(args, 0);
        Ok(RubyValue::Int(ptr_of(recv).address() as i64))
    }
    "size" | "total" => fn size(recv, args, _b) {
        arity!(args, 0);
        Ok(RubyValue::Int(ptr_of(recv).size.unwrap_or(0) as i64))
    }
    "+" => fn add(recv, args, _b) {
        arity!(args, 1);
        let delta = crate::ffi::to_i64(&args[0])? as usize;
        Ok(RubyValue::Object(Arc::new(ptr_of(recv).offset(delta))))
    }
    "==" | "eql?" => fn eq(recv, args, _b) {
        arity!(args, 1);
        Ok(RubyValue::Bool(address_of(&args[0]) == Some(ptr_of(recv).address())))
    }
    "slice" => fn slice(recv, args, _b) {
        arity!(args, 2);
        let off = crate::ffi::to_i64(&args[0])? as usize;
        let len = crate::ffi::to_i64(&args[1])? as usize;
        let mut p = ptr_of(recv).offset(off);
        p.size = Some(len);
        Ok(RubyValue::Object(Arc::new(p)))
    }
}

/// `read_array_of_<int>(count)` -> an `Array` of `count` integers.
fn read_int_array(
    recv: &RubyValue,
    args: &[RubyValue],
    bytes: usize,
    signed: bool,
) -> Result<RubyValue, Signal> {
    arity!(args, 1);
    let n = crate::ffi::to_i64(&args[0])? as usize;
    let p = ptr_of(recv);
    p.check_bounds(0, n * bytes)?;
    let out: Vec<RubyValue> = (0..n)
        .map(|i| RubyValue::Int(unsafe { p.read_int(i * bytes, bytes, signed) }))
        .collect();
    Ok(RubyValue::Array(crate::array_new(out)))
}

/// `write_array_of_<int>(array)` -- writes each element sequentially.
fn write_int_array(
    recv: &RubyValue,
    args: &[RubyValue],
    bytes: usize,
) -> Result<RubyValue, Signal> {
    arity!(args, 1);
    let elems = array_elems(&args[0])?;
    let p = ptr_of(recv);
    p.check_bounds(0, elems.len() * bytes)?;
    for (i, e) in elems.iter().enumerate() {
        unsafe { p.write_int(i * bytes, bytes, crate::ffi::to_i64(e)?) };
    }
    Ok(recv.clone())
}

fn read_float_array(
    recv: &RubyValue,
    args: &[RubyValue],
    bytes: usize,
) -> Result<RubyValue, Signal> {
    arity!(args, 1);
    let n = crate::ffi::to_i64(&args[0])? as usize;
    let p = ptr_of(recv);
    p.check_bounds(0, n * bytes)?;
    let out: Vec<RubyValue> = (0..n)
        .map(|i| RubyValue::Float(unsafe { p.read_float(i * bytes, bytes) }))
        .collect();
    Ok(RubyValue::Array(crate::array_new(out)))
}

fn write_float_array(
    recv: &RubyValue,
    args: &[RubyValue],
    bytes: usize,
) -> Result<RubyValue, Signal> {
    arity!(args, 1);
    let elems = array_elems(&args[0])?;
    let p = ptr_of(recv);
    p.check_bounds(0, elems.len() * bytes)?;
    for (i, e) in elems.iter().enumerate() {
        unsafe { p.write_float(i * bytes, bytes, crate::ffi::to_f64(e)?) };
    }
    Ok(recv.clone())
}

fn array_elems(v: &RubyValue) -> Result<Vec<RubyValue>, Signal> {
    Ok(crate::builtins::convert::to_rary(v)?
        .lock()
        .iter()
        .cloned()
        .collect())
}

// ---- FFI::Pointer class methods ----

builtin_methods! {
    pub(crate) fn lookup_class_pointer;

    // `FFI::Pointer.new(address)` or `FFI::Pointer.new(type, address)` (the
    // type governs `[]` element size, which we don't model -- the address is
    // what matters). A Pointer argument copies its address.
    "new" => fn new_pointer(_recv, args, _b) {
        arity!(args, 1..=2);
        let addr_arg = args.last().expect("arity checked");
        let addr = match address_of(addr_arg) {
            Some(a) => a,
            None => crate::ffi::to_i64(addr_arg)? as usize,
        };
        Ok(RubyValue::Object(Arc::new(RPointer::raw(addr, FFI_POINTER_CLASS))))
    }
}

// ---- FFI::MemoryPointer class methods ----

builtin_methods! {
    pub(crate) fn lookup_class_memory;

    // `MemoryPointer.new(type, count = 1, clear = true)`. `type` is a type
    // symbol (`:int` -> 4 bytes) or an Integer element size in bytes. A block
    // form yields the pointer and returns the block's value (the gem also
    // auto-frees afterward; our pointer is GC-managed, so the buffer simply
    // lives as long as it is referenced).
    "new" => fn new_memory_m(_recv, args, block) {
        arity!(args, 1..=3);
        let elem = memptr_elem_size(&args[0])?;
        let count = match args.get(1) {
            None | Some(RubyValue::Nil) => 1,
            Some(v) => crate::ffi::to_i64(v)? as usize,
        };
        let ptr = new_memory(elem * count);
        match block {
            Some(p @ RubyValue::Proc(_)) => {
                crate::dispatch::send_value(&p, Symbol::intern("call"), &[ptr], None)
            }
            _ => Ok(ptr),
        }
    }
    // `MemoryPointer.from_string(str)` -- an owned buffer holding the bytes + NUL.
    "from_string" => fn from_string(_recv, args, _b) {
        arity!(args, 1);
        let bytes = str_bytes(&args[0])?;
        Ok(RubyValue::Object(Arc::new(RPointer::from_bytes_nul(&bytes))))
    }
}

/// The element size (bytes) for a `MemoryPointer.new` first argument: a type
/// symbol maps by `type_size`, an Integer is a literal byte size.
fn memptr_elem_size(v: &RubyValue) -> Result<usize, Signal> {
    match v {
        RubyValue::Int(n) => Ok(*n as usize),
        RubyValue::Symbol(s) => {
            type_size(&s.name()).ok_or_else(|| arg_error!("unknown FFI type {}", s.name()))
        }
        other => Err(type_error!(
            "cannot derive a size from {}",
            crate::builtins::class_name_of(other)
        )),
    }
}

/// The byte size of an `ffi` scalar type keyword (LP64). Shared by
/// `MemoryPointer.new(:int, ...)` and any type-sized computation.
pub fn type_size(sym: &str) -> Option<usize> {
    Some(match sym {
        "char" | "uchar" | "int8" | "uint8" | "bool" => 1,
        "short" | "ushort" | "int16" | "uint16" => 2,
        "int" | "uint" | "int32" | "uint32" | "float" => 4,
        "long" | "ulong" | "int64" | "uint64" | "long_long" | "ulong_long" | "size_t"
        | "ssize_t" | "double" | "pointer" => 8,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(recv: &RubyValue, name: &str, args: &[RubyValue]) -> RubyValue {
        lookup(name).expect("method exists")(recv, args, None).expect("no error")
    }
    fn as_int(v: RubyValue) -> i64 {
        match v {
            RubyValue::Int(i) => i,
            other => panic!("expected Int, got {other:?}"),
        }
    }
    fn as_bool(v: RubyValue) -> bool {
        match v {
            RubyValue::Bool(b) => b,
            other => panic!("expected Bool, got {other:?}"),
        }
    }

    #[test]
    fn memory_pointer_round_trips_ints() {
        let p = new_memory(12);
        call(&p, "put_int", &[RubyValue::Int(0), RubyValue::Int(10)]);
        call(&p, "put_int", &[RubyValue::Int(4), RubyValue::Int(20)]);
        assert_eq!(as_int(call(&p, "get_int", &[RubyValue::Int(0)])), 10);
        assert_eq!(as_int(call(&p, "get_int", &[RubyValue::Int(4)])), 20);
        assert_eq!(as_int(call(&p, "size", &[])), 12);
        assert!(!as_bool(call(&p, "null?", &[])));
    }

    #[test]
    fn write_then_read_at_offset_zero() {
        let p = new_memory(8);
        call(&p, "write_int", &[RubyValue::Int(99)]);
        assert_eq!(as_int(call(&p, "read_int", &[])), 99);
    }

    #[test]
    fn pointer_arithmetic_shares_the_buffer() {
        let p = new_memory(12);
        call(&p, "put_int", &[RubyValue::Int(4), RubyValue::Int(20)]);
        let q = call(&p, "+", &[RubyValue::Int(4)]);
        assert_eq!(as_int(call(&q, "read_int", &[])), 20);
    }

    #[test]
    fn array_round_trip() {
        let p = new_memory(12);
        let arr = RubyValue::Array(crate::array_new(vec![
            RubyValue::Int(1),
            RubyValue::Int(2),
            RubyValue::Int(3),
        ]));
        call(&p, "write_array_of_int", &[arr]);
        let out = call(&p, "read_array_of_int", &[RubyValue::Int(3)]);
        let RubyValue::Array(a) = out else {
            panic!("expected array")
        };
        let got: Vec<i64> = a.lock().iter().cloned().map(as_int).collect();
        assert_eq!(got, vec![1, 2, 3]);
    }

    // Out-of-bounds -> `IndexError` needs the exception REGISTRY, absent in a
    // unit test (`raise_error` panics without it); it is covered by the e2e
    // suite where the full runtime is linked.

    #[test]
    fn null_pointer_reports_null() {
        let p = wrap_address(0);
        assert!(as_bool(call(&p, "null?", &[])));
        assert_eq!(as_int(call(&p, "address", &[])), 0);
    }

    #[test]
    fn type_sizes_match_lp64() {
        assert_eq!(type_size("int"), Some(4));
        assert_eq!(type_size("long"), Some(8));
        assert_eq!(type_size("char"), Some(1));
        assert_eq!(type_size("double"), Some(8));
        assert_eq!(type_size("pointer"), Some(8));
        assert_eq!(type_size("nope"), None);
    }
}
