//! The `ffi` gem's memory classes: `FFI::Pointer` and `FFI::MemoryPointer`.
//! Unlike the `attach_function` machinery (which is a pure compile-time
//! `extern "C"` emission -- see `zeo`'s `emit_ffi_call` and this crate's
//! `crate::ffi` marshaling), these are real runtime values that flow through
//! Ruby code: a `MemoryPointer` allocates a heap buffer and answers
//! `read_int`/`write_int`/`[]`/`+`, and a `:pointer` return from a C function is
//! wrapped as an `FFI::Pointer`. The surface is oracle-verified against
//! `ffi 1.17.4`.
//!
//! This module holds the shared payload (`RPointer` + its owned-buffer memory
//! model) and the read/write helpers; the two classes each live in their own
//! file. `MemoryPointer < Pointer` in the ABI, so it needs no instance table of
//! its own -- the ancestry walk reaches `Pointer`'s registered methods -- and
//! contributes only its class methods (`new`/`from_string`).
//!
//! Memory model: an `RPointer` is a raw C address (`base`) plus an optional
//! owned heap buffer (`owner`) that keeps that address alive. A `MemoryPointer`
//! owns its buffer; a `Pointer` wrapping a raw address (`Pointer.new(addr)`, or
//! a C return) does not. Pointer arithmetic (`ptr + n`) shares the owner and
//! advances the address, exactly like the gem. Reads/writes are raw unaligned
//! accesses, so the same code serves an owned Ruby buffer and foreign C memory.
//!
//! The unsigned 64-bit boundary is a filed gap, not a decision:
//! `tests/gaps/ffi_unsigned_64_bit_values.rb`. Integer HAS a Bignum tier
//! now, so the old note's reason ("no Bignum tier, so it wraps negative")
//! no longer holds -- and the write side does not wrap, it refuses.
//!
//! `#address`/`#inspect` expose a real heap address for an owned buffer, so
//! they are non-deterministic and never golden-tested.

mod auto_pointer;
mod dynamic_library;
mod function;
mod memory_pointer;
mod pointer;
mod types;

use crate::builtins::{arg_error, index_error, type_error};
use crate::dispatch::{RObj, RubyObject, raise_error};
use crate::{ClassId, RubyValue, Signal};
use std::alloc::{Layout, alloc_zeroed, dealloc};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

/// An `FFI::Pointer` / `FFI::MemoryPointer` / `FFI::Function` instance.
pub struct RPointer {
    /// The real C address this points at (owned buffer data, or a raw address).
    base: *mut u8,
    /// Byte size when known (owned buffers); `None` for a raw address whose
    /// extent the gem also doesn't know.
    size: Option<usize>,
    /// The owned buffer keeping `base` alive; `None` for a raw address.
    owner: Option<Arc<OwnedBuf>>,
    /// `FFI_POINTER_CLASS`, `FFI_MEMORY_POINTER_CLASS` or
    /// `FFI_FUNCTION_CLASS` -- governs `.class`.
    class: ClassId,
    /// The `FFI::Function` payload (call signature + any live Proc closure).
    /// An `FFI::Function` IS a `Pointer` in the gem's hierarchy, so it shares
    /// this struct -- the whole `Pointer` instance table works on it unchanged.
    func: Option<Arc<function::FuncData>>,
    frozen: AtomicBool,
    /// `#autorelease?` -- whether this pointer's memory is released with the
    /// object. True exactly when the object OWNS the memory, which is what
    /// makes a `MemoryPointer` answer true and a raw `Pointer` false (both
    /// oracle-verified). Settable, as the gem's is. zeo runs no finalizer,
    /// so the flag is reported faithfully and `#free` is the release that
    /// actually happens -- see `Pointer#free`.
    pub(super) autorelease: AtomicBool,
    /// `MemoryPointer#type_size` -- the element size `new(:int, n)` was given,
    /// so `#+ type_size` steps one element. 1 for a pointer minted from a raw
    /// address or a byte count, which is what the gem answers there too.
    pub(super) type_size: AtomicUsize,
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
            func: None,
            frozen: AtomicBool::new(false),
            autorelease: AtomicBool::new(true),
            type_size: AtomicUsize::new(1),
        }
    }

    /// An owned buffer holding `bytes` followed by a NUL (`from_string`).
    fn from_bytes_nul(bytes: &[u8]) -> RPointer {
        let p = RPointer::owned(bytes.len() + 1, FFI_MEMORY_POINTER_CLASS);
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), p.base, bytes.len()) };
        p
    }

    /// A `Pointer` over a raw address it does not own.
    /// [`RPointer::raw`] for the sibling class tables -- `AutoPointer.new`
    /// builds its own pointer and then flips `autorelease`.
    pub(super) fn raw_at(addr: usize, class: ClassId) -> RPointer {
        RPointer::raw(addr, class)
    }

    fn raw(addr: usize, class: ClassId) -> RPointer {
        RPointer {
            base: addr as *mut u8,
            size: None,
            owner: None,
            class,
            func: None,
            frozen: AtomicBool::new(false),
            autorelease: AtomicBool::new(false),
            type_size: AtomicUsize::new(1),
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
            func: None,
            frozen: AtomicBool::new(false),
            autorelease: AtomicBool::new(false),
            type_size: AtomicUsize::new(1),
        }
    }

    fn address(&self) -> usize {
        self.base as usize
    }

    /// Reject an out-of-bounds access on a sized (owned) buffer, exactly where
    /// the gem raises `IndexError`; a raw address (unknown size) is unchecked.
    /// A NULL pointer rejects every non-empty access first, as the gem's
    /// `FFI::NullPointerError` does (the class itself is defined by the ffi
    /// gem's Ruby half, `ext/ffi`, so it is raisable by name).
    fn check_bounds(&self, off: usize, len: usize) -> Result<(), Signal> {
        if len > 0 && self.base.is_null() {
            return Err(null_pointer_error());
        }
        if let Some(size) = self.size
            && off + len > size
        {
            return Err(index_error!(
                "Memory access offset={off} size={len} out of bounds (total {size})"
            ));
        }
        Ok(())
    }

    /// Read `n` raw bytes at `off`. A zero-length read never touches the
    /// address, so it is valid even on NULL (fiddle's `NULL.to_str` -> `""`).
    unsafe fn read_bytes_at(&self, off: usize, n: usize) -> Vec<u8> {
        if n == 0 {
            return Vec::new();
        }
        unsafe { std::slice::from_raw_parts(self.base.add(off), n) }.to_vec()
    }

    /// Write `bytes` at `off`; empty writes never touch the address.
    unsafe fn write_bytes_at(&self, off: usize, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), self.base.add(off), bytes.len()) };
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
            func: self.func.clone(),
            frozen: AtomicBool::new(false),
            type_size: AtomicUsize::new(self.type_size.load(Ordering::Relaxed)),
            // A dup is a second handle onto the SAME memory, so it must not
            // claim ownership of it -- the gem's `#dup` answers false too.
            autorelease: AtomicBool::new(false),
        };
        if copy_frozen {
            p.set_frozen();
        }
        Arc::new(p)
    }
}

/// The gem's `FFI::NullPointerError` -- defined by the ffi gem's Ruby half
/// (`ext/ffi/lib/ffi.rb`), which every `require "ffi"` loads, so raising it
/// by name from here works (the `strscan` pattern; see `ext/mod.rs`).
fn null_pointer_error() -> Signal {
    raise_error(
        "FFI::NullPointerError",
        "invalid memory access at address=0x0".to_string(),
    )
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

/// A fresh `MemoryPointer` holding a COPY of the `len` bytes at `src` -- how
/// a struct returned BY VALUE becomes the ruby-owned backing an `FFI::Struct`
/// then views (the generated call site passes a reference to the C return
/// value it just received, and wraps this in the struct's own class).
///
/// # Safety
/// `src..src+len` must be readable for the duration of the call.
pub unsafe fn memory_from_bytes(src: *const u8, len: usize) -> RubyValue {
    let mp = RPointer::owned(len, FFI_MEMORY_POINTER_CLASS);
    unsafe { std::ptr::copy_nonoverlapping(src, mp.base, len) };
    RubyValue::Object(Arc::new(mp))
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
fn off_arg(offset: Option<&RubyValue>) -> Result<usize, Signal> {
    match offset {
        None => Ok(0),
        Some(v) => Ok(crate::ffi::to_i64(v)? as usize),
    }
}

/// `read_*` (offset 0) and `get_*(offset)` share this; the two differ only in
/// where `off` comes from, which each def's parameter list now says.
fn read_int_m(
    recv: &RubyValue,
    off: usize,
    bytes: usize,
    signed: bool,
) -> Result<RubyValue, Signal> {
    let p = ptr_of(recv);
    p.check_bounds(off, bytes)?;
    Ok(RubyValue::Int(unsafe { p.read_int(off, bytes, signed) }))
}

/// `write_*(value)` (offset 0) / `put_*(offset, value)`.
fn write_int_m(
    recv: &RubyValue,
    off: usize,
    value: &RubyValue,
    bytes: usize,
) -> Result<RubyValue, Signal> {
    let v = crate::ffi::to_i64(value)?;
    let p = ptr_of(recv);
    p.check_bounds(off, bytes)?;
    unsafe { p.write_int(off, bytes, v) };
    Ok(recv.clone())
}

fn read_float_m(recv: &RubyValue, off: usize, bytes: usize) -> Result<RubyValue, Signal> {
    let p = ptr_of(recv);
    p.check_bounds(off, bytes)?;
    Ok(RubyValue::Float(unsafe { p.read_float(off, bytes) }))
}

fn write_float_m(
    recv: &RubyValue,
    off: usize,
    value: &RubyValue,
    bytes: usize,
) -> Result<RubyValue, Signal> {
    let v = crate::ffi::to_f64(value)?;
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
    // ASCII-8BIT, not the default external: every ffi read hands back raw C
    // bytes, and ruby-ffi tags them BINARY (oracle-verified on `read_string`,
    // `read_bytes`, `get_string`, `get_bytes` and `read_array_of_string`).
    RubyValue::Str(crate::string_from_bytes(bytes, crate::encoding::ASCII_8BIT))
}

/// `read_array_of_<int>(count)` / `get_array_of_<int>(offset, count)` -> an
/// `Array` of `count` integers. The `read_` spelling is the `off = 0` case,
/// exactly as it is for the scalar accessors.
fn read_int_array(
    recv: &RubyValue,
    off: usize,
    count: &RubyValue,
    bytes: usize,
    signed: bool,
) -> Result<RubyValue, Signal> {
    let n = crate::ffi::to_i64(count)? as usize;
    let p = ptr_of(recv);
    p.check_bounds(off, n * bytes)?;
    let out: Vec<RubyValue> = (0..n)
        .map(|i| RubyValue::Int(unsafe { p.read_int(off + i * bytes, bytes, signed) }))
        .collect();
    Ok(RubyValue::Array(crate::array_new(out)))
}

/// `write_array_of_<int>(array)` / `put_array_of_<int>(offset, array)` --
/// writes each element sequentially.
fn write_int_array(
    recv: &RubyValue,
    off: usize,
    ary: &RubyValue,
    bytes: usize,
) -> Result<RubyValue, Signal> {
    let elems = array_elems(ary)?;
    let p = ptr_of(recv);
    p.check_bounds(off, elems.len() * bytes)?;
    for (i, e) in elems.iter().enumerate() {
        unsafe { p.write_int(off + i * bytes, bytes, crate::ffi::to_i64(e)?) };
    }
    Ok(recv.clone())
}

fn read_float_array(
    recv: &RubyValue,
    off: usize,
    count: &RubyValue,
    bytes: usize,
) -> Result<RubyValue, Signal> {
    let n = crate::ffi::to_i64(count)? as usize;
    let p = ptr_of(recv);
    p.check_bounds(off, n * bytes)?;
    let out: Vec<RubyValue> = (0..n)
        .map(|i| RubyValue::Float(unsafe { p.read_float(off + i * bytes, bytes) }))
        .collect();
    Ok(RubyValue::Array(crate::array_new(out)))
}

fn write_float_array(
    recv: &RubyValue,
    off: usize,
    ary: &RubyValue,
    bytes: usize,
) -> Result<RubyValue, Signal> {
    let elems = array_elems(ary)?;
    let p = ptr_of(recv);
    p.check_bounds(off, elems.len() * bytes)?;
    for (i, e) in elems.iter().enumerate() {
        unsafe { p.write_float(off + i * bytes, bytes, crate::ffi::to_f64(e)?) };
    }
    Ok(recv.clone())
}

/// `read_array_of_pointer(count)` / `get_array_of_pointer(offset, count)` --
/// each slot is a raw address, wrapped as an `FFI::Pointer`.
fn read_pointer_array(
    recv: &RubyValue,
    off: usize,
    count: &RubyValue,
) -> Result<RubyValue, Signal> {
    let n = crate::ffi::to_i64(count)? as usize;
    let p = ptr_of(recv);
    p.check_bounds(off, n * 8)?;
    let out: Vec<RubyValue> = (0..n)
        .map(|i| wrap_address(unsafe { p.read_int(off + i * 8, 8, false) } as usize))
        .collect();
    Ok(RubyValue::Array(crate::array_new(out)))
}

fn write_pointer_array(recv: &RubyValue, off: usize, ary: &RubyValue) -> Result<RubyValue, Signal> {
    let elems = array_elems(ary)?;
    let p = ptr_of(recv);
    p.check_bounds(off, elems.len() * 8)?;
    for (i, e) in elems.iter().enumerate() {
        let addr = address_of(e).unwrap_or(0);
        unsafe { p.write_int(off + i * 8, 8, addr as i64) };
    }
    Ok(recv.clone())
}

/// `read_array_of_string(count)` / `get_array_of_string(offset, count)` -- an
/// array of `char *`, each read as a NUL-terminated string (a NULL slot is
/// `nil`). ruby-ffi has no `put_` twin for this one.
fn read_string_array(
    recv: &RubyValue,
    off: usize,
    count: Option<&RubyValue>,
) -> Result<RubyValue, Signal> {
    let p = ptr_of(recv);
    // No count: read until the first NULL slot, ruby-ffi's own argv rule.
    let n = match count {
        Some(c) => crate::ffi::to_i64(c)? as usize,
        None => {
            let mut n = 0usize;
            loop {
                p.check_bounds(off + n * 8, 8)?;
                if unsafe { p.read_int(off + n * 8, 8, false) } == 0 {
                    break n;
                }
                n += 1;
            }
        }
    };
    p.check_bounds(off, n * 8)?;
    let out: Vec<RubyValue> = (0..n)
        .map(|i| {
            let addr = unsafe { p.read_int(off + i * 8, 8, false) } as usize;
            if addr == 0 {
                RubyValue::Nil
            } else {
                bytes_to_str(unsafe { c_string_bytes(addr) })
            }
        })
        .collect();
    Ok(RubyValue::Array(crate::array_new(out)))
}

/// The NUL-terminated bytes at a raw address.
///
/// # Safety
/// `addr` must point at a NUL-terminated C string the caller owns or
/// borrows -- the same contract every `:string` return already relies on.
unsafe fn c_string_bytes(addr: usize) -> Vec<u8> {
    unsafe { std::ffi::CStr::from_ptr(addr as *const libc::c_char) }
        .to_bytes()
        .to_vec()
}

/// `read_array_of_type(type, :reader, n)` -- ruby-ffi writes this one in Ruby,
/// over the very accessors above; it names the reader as a Symbol and steps by
/// the type's own size.
fn read_typed_array(
    recv: &RubyValue,
    reader: &str,
    bytes: usize,
    length: &RubyValue,
) -> Result<RubyValue, Signal> {
    let n = crate::ffi::to_i64(length)? as usize;
    let sym = crate::Symbol::intern(reader);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let slot = RubyValue::Object(Arc::new(ptr_of(recv).offset(i * bytes)));
        out.push(crate::dispatch::send_value(&slot, sym, &[], None)?);
    }
    Ok(RubyValue::Array(crate::array_new(out)))
}

/// The writer twin -- and NOT symmetric with the reader: ruby-ffi's own
/// `write_array_of_type` sends `writer(i * size, val)` to SELF, so the method
/// it names is the offset-taking `put_` spelling while the reader's is the
/// offset-0 `read_` one (`pointer.rb`, lines 128-138).
fn write_typed_array(
    recv: &RubyValue,
    writer: &str,
    bytes: usize,
    ary: &RubyValue,
) -> Result<RubyValue, Signal> {
    let elems = array_elems(ary)?;
    let sym = crate::Symbol::intern(writer);
    for (i, e) in elems.iter().enumerate() {
        let args = [RubyValue::Int((i * bytes) as i64), e.clone()];
        crate::dispatch::send_value(recv, sym, &args, None)?;
    }
    Ok(recv.clone())
}

/// Record the element size `MemoryPointer.new(:int, n)` was built with, which
/// is what `#type_size` answers back.
fn set_type_size(v: &RubyValue, elem: usize) {
    ptr_of(v).type_size.store(elem, Ordering::Relaxed);
}

/// A method NAME argument spelled either way (`:read_int` / `"read_int"`).
fn method_name_arg(v: &RubyValue) -> Result<String, Signal> {
    match v {
        RubyValue::Symbol(s) => Ok(s.name().to_string()),
        RubyValue::Str(s) => Ok(s.lock().to_utf8_lossy().into_owned()),
        other => Err(type_error!(
            "{} is not a symbol nor a string",
            crate::builtins::class_name_of(other)
        )),
    }
}

fn array_elems(v: &RubyValue) -> Result<Vec<RubyValue>, Signal> {
    Ok(crate::builtins::convert::to_rary(v)?
        .lock()
        .iter()
        .cloned()
        .collect())
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

/// The byte size of an `ffi` scalar type keyword (LP64) -- the shared
/// `CScalar` table. Shared by `MemoryPointer.new(:int, ...)` and any
/// type-sized computation. `:string`/`:buffer_*` size as the pointers they
/// are, matching the gem's own `FFI.type_size`; `:void` has no element size.
pub fn type_size(sym: &str) -> Option<usize> {
    zeo_abi::ffi::CScalar::from_keyword(sym)
        .filter(|s| !matches!(s, zeo_abi::ffi::CScalar::Void))
        .map(|s| s.size())
}

use crate::errno_ptr;

zeo_macros::ruby_module! {
    FFI = zeo_abi::FFI_MODULE;

    // `FFI.errno` / `FFI.errno=` -- the saved C errno; fiddle's `last_error`
    // reads through this.
    def self."errno"(_recv) {
        Ok(RubyValue::Int(unsafe { *errno_ptr() } as i64))
    }
    def self."errno="(_recv, arg) {
        let v = crate::ffi::to_i64(arg)?;
        unsafe { *errno_ptr() = v as libc::c_int };
        Ok((*arg).clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtins::registered_table;

    fn call(recv: &RubyValue, name: &str, args: &[RubyValue]) -> RubyValue {
        let lookup = registered_table(FFI_POINTER_CLASS)
            .and_then(|t| t.instance.as_ref())
            .expect("Pointer registers an instance table")
            .lookup;
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
