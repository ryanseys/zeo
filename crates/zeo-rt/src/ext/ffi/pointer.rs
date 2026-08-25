//! `FFI::Pointer < Object` -- the read/write surface over a raw C address
//! (`read_int`/`get_int`/`write_pointer`/`read_string`/`+`/`==`/...). Every
//! `MemoryPointer` inherits these through the ancestry walk; this file owns the
//! whole instance table plus `Pointer.new(address)`.

use std::os::raw::c_char;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::{
    RPointer, address_of, bytes_to_str, memptr_elem_size, method_name_arg, off_arg, ptr_of,
    read_float_array, read_float_m, read_int_array, read_int_m, read_pointer_array,
    read_string_array, read_typed_array, str_bytes, wrap_address, write_float_array, write_float_m,
    write_int_array, write_int_m, write_pointer_array, write_typed_array,
};
use crate::builtins::{index_error, type_error};
use crate::{RubyValue, Symbol};
use zeo_abi::FFI_POINTER_CLASS;
use zeo_macros::ruby_class;

ruby_class! {
    Pointer = zeo_abi::FFI_POINTER_CLASS < zeo_abi::FFI_ABSTRACT_MEMORY_CLASS;

    // `FFI::Pointer.new(address)` or `FFI::Pointer.new(type, address)` (the
    // type governs `[]` element size, which we don't model -- the address is
    // what matters). A Pointer argument copies its address.
    def self."new" cfunc (_recv, type_or_address, address?) {
        let addr_arg = address.unwrap_or(type_or_address);
        let addr = match address_of(addr_arg) {
            Some(a) => a,
            None => crate::ffi::to_i64(addr_arg)? as usize,
        };
        Ok(RubyValue::Object(Arc::new(RPointer::raw(addr, FFI_POINTER_CLASS))))
    }

    // -- signed/unsigned integer reads (offset 0) --
    def "read_int8" | "read_char"(r) { read_int_m(r, 0, 1, true) }
    def "read_uint8" | "read_uchar"(r) { read_int_m(r, 0, 1, false) }
    def "read_int16" | "read_short"(r) { read_int_m(r, 0, 2, true) }
    def "read_uint16" | "read_ushort"(r) { read_int_m(r, 0, 2, false) }
    def "read_int32" | "read_int"(r) { read_int_m(r, 0, 4, true) }
    def "read_uint32" | "read_uint"(r) { read_int_m(r, 0, 4, false) }
    def "read_int64" | "read_long" | "read_long_long"(r) { read_int_m(r, 0, 8, true) }
    def "read_uint64" | "read_ulong" | "read_ulong_long"(r) { read_int_m(r, 0, 8, false) }

    // -- integer reads at an offset --
    def "get_int8" | "get_char"(r, offset) { read_int_m(r, off_arg(Some(offset))?, 1, true) }
    def "get_uint8" | "get_uchar"(r, offset) { read_int_m(r, off_arg(Some(offset))?, 1, false) }
    def "get_int16" | "get_short"(r, offset) { read_int_m(r, off_arg(Some(offset))?, 2, true) }
    def "get_uint16" | "get_ushort"(r, offset) { read_int_m(r, off_arg(Some(offset))?, 2, false) }
    def "get_int32" | "get_int"(r, offset) { read_int_m(r, off_arg(Some(offset))?, 4, true) }
    def "get_uint32" | "get_uint"(r, offset) { read_int_m(r, off_arg(Some(offset))?, 4, false) }
    def "get_int64" | "get_long" | "get_long_long"(r, offset) { read_int_m(r, off_arg(Some(offset))?, 8, true) }
    def "get_uint64" | "get_ulong" | "get_ulong_long"(r, offset) { read_int_m(r, off_arg(Some(offset))?, 8, false) }

    // -- integer writes (offset 0) --
    def "write_int8" | "write_char"(r, value) { write_int_m(r, 0, value, 1) }
    def "write_uint8" | "write_uchar"(r, value) { write_int_m(r, 0, value, 1) }
    def "write_int16" | "write_short"(r, value) { write_int_m(r, 0, value, 2) }
    def "write_uint16" | "write_ushort"(r, value) { write_int_m(r, 0, value, 2) }
    def "write_int32" | "write_int"(r, value) { write_int_m(r, 0, value, 4) }
    def "write_uint32" | "write_uint"(r, value) { write_int_m(r, 0, value, 4) }
    def "write_int64" | "write_long" | "write_long_long"(r, value) { write_int_m(r, 0, value, 8) }
    def "write_uint64" | "write_ulong" | "write_ulong_long"(r, value) { write_int_m(r, 0, value, 8) }

    // -- integer writes at an offset --
    def "put_int8" | "put_char"(r, offset, value) { write_int_m(r, off_arg(Some(offset))?, value, 1) }
    def "put_uint8" | "put_uchar"(r, offset, value) { write_int_m(r, off_arg(Some(offset))?, value, 1) }
    def "put_int16" | "put_short"(r, offset, value) { write_int_m(r, off_arg(Some(offset))?, value, 2) }
    def "put_uint16" | "put_ushort"(r, offset, value) { write_int_m(r, off_arg(Some(offset))?, value, 2) }
    def "put_int32" | "put_int"(r, offset, value) { write_int_m(r, off_arg(Some(offset))?, value, 4) }
    def "put_uint32" | "put_uint"(r, offset, value) { write_int_m(r, off_arg(Some(offset))?, value, 4) }
    def "put_int64" | "put_long" | "put_long_long"(r, offset, value) { write_int_m(r, off_arg(Some(offset))?, value, 8) }
    def "put_uint64" | "put_ulong" | "put_ulong_long"(r, offset, value) { write_int_m(r, off_arg(Some(offset))?, value, 8) }

    // -- floats --
    def "read_float"(r) { read_float_m(r, 0, 4) }
    def "read_double"(r) { read_float_m(r, 0, 8) }
    def "get_float32" | "get_float"(r, offset) { read_float_m(r, off_arg(Some(offset))?, 4) }
    def "get_float64" | "get_double"(r, offset) { read_float_m(r, off_arg(Some(offset))?, 8) }
    def "write_float"(r, value) { write_float_m(r, 0, value, 4) }
    def "write_double"(r, value) { write_float_m(r, 0, value, 8) }
    def "put_float32" | "put_float"(r, offset, value) { write_float_m(r, off_arg(Some(offset))?, value, 4) }
    def "put_float64" | "put_double"(r, offset, value) { write_float_m(r, off_arg(Some(offset))?, value, 8) }

    // -- pointers (read/write an address-sized word, wrapped as a Pointer) --
    def "read_pointer" arity 0 | "get_pointer" arity 1 (recv, offset?) {
        let off = off_arg(offset)?;
        let p = ptr_of(recv);
        p.check_bounds(off, 8)?;
        Ok(wrap_address(unsafe { p.read_int(off, 8, false) } as usize))
    }
    def "write_pointer" arity 1 | "put_pointer" arity 2 (recv, first, second?) {
        let (off, target) = match second {
            Some(v) => (off_arg(Some(first))?, v),
            None => (0, first),
        };
        let addr = address_of(target).ok_or_else(|| type_error!("wrong argument type (expected a pointer)"))?;
        let p = ptr_of(recv);
        p.check_bounds(off, 8)?;
        unsafe { p.write_int(off, 8, addr as i64) };
        Ok(recv.clone())
    }

    // -- strings & raw bytes --
    // `read_string` -> up to the first NUL; `read_string(len)` -> exactly len bytes.
    def "read_string"(recv, arg?) {
        let p = ptr_of(recv);
        match arg {
            None | Some(RubyValue::Nil) => {
                p.check_bounds(0, 1)?;
                let bytes = unsafe { std::ffi::CStr::from_ptr(p.base as *const c_char) }.to_bytes().to_vec();
                Ok(bytes_to_str(bytes))
            }
            Some(len) => {
                let n = crate::ffi::to_i64(len)? as usize;
                p.check_bounds(0, n)?;
                Ok(bytes_to_str(unsafe { p.read_bytes_at(0, n) }))
            }
        }
    }
    // `get_string(offset, length = nil)` -- NUL-terminated at offset, or fixed length.
    def "get_string" cfunc (recv, arg1, arg2?) {
        let off = crate::ffi::to_i64(arg1)? as usize;
        let p = ptr_of(recv);
        match arg2 {
            None | Some(RubyValue::Nil) => {
                p.check_bounds(off, 1)?;
                let bytes = unsafe { std::ffi::CStr::from_ptr(p.base.add(off) as *const c_char) }.to_bytes().to_vec();
                Ok(bytes_to_str(bytes))
            }
            Some(len) => {
                // Bounded NUL-terminated read: up to `len` bytes, stopping at
                // the first NUL (the gem's `get_string`; `read_string(len)`
                // stays exact).
                let n = crate::ffi::to_i64(len)? as usize;
                p.check_bounds(off, n)?;
                let mut bytes = unsafe { p.read_bytes_at(off, n) };
                if let Some(nul) = bytes.iter().position(|&b| b == 0) {
                    bytes.truncate(nul);
                }
                Ok(bytes_to_str(bytes))
            }
        }
    }
    // `put_string(offset, str)` writes the bytes plus a terminating NUL.
    def "put_string"(recv, arg1, arg2) {
        let off = crate::ffi::to_i64(arg1)? as usize;
        let bytes = str_bytes(arg2)?;
        let p = ptr_of(recv);
        p.check_bounds(off, bytes.len() + 1)?;
        unsafe {
            p.write_bytes_at(off, &bytes);
            p.base.add(off + bytes.len()).write(0);
        }
        Ok(recv.clone())
    }
    // `read_bytes(len)` / `get_bytes(offset, len)` -- raw bytes, NUL-agnostic.
    def "read_bytes"(recv, arg) {
        let n = crate::ffi::to_i64(arg)? as usize;
        let p = ptr_of(recv);
        p.check_bounds(0, n)?;
        Ok(bytes_to_str(unsafe { p.read_bytes_at(0, n) }))
    }
    def "get_bytes"(recv, arg1, arg2) {
        let off = crate::ffi::to_i64(arg1)? as usize;
        let n = crate::ffi::to_i64(arg2)? as usize;
        let p = ptr_of(recv);
        p.check_bounds(off, n)?;
        Ok(bytes_to_str(unsafe { p.read_bytes_at(off, n) }))
    }
    // `put_bytes(offset, str, index = 0, length = nil)` -- raw bytes (a slice
    // of `str` starting at `index`), no NUL.
    def "put_bytes" cfunc (recv, arg1, arg2, arg3?, arg4?) {
        let off = crate::ffi::to_i64(arg1)? as usize;
        let mut bytes = str_bytes(arg2)?;
        let idx = match arg3 {
            None | Some(RubyValue::Nil) => 0,
            Some(v) => crate::ffi::to_i64(v)? as usize,
        };
        let len = match arg4 {
            None | Some(RubyValue::Nil) => bytes.len().saturating_sub(idx),
            Some(v) => crate::ffi::to_i64(v)? as usize,
        };
        if idx + len > bytes.len() {
            return Err(index_error!(
                "index {idx} or length {len} is out of string"
            ));
        }
        bytes = bytes[idx..idx + len].to_vec();
        let p = ptr_of(recv);
        p.check_bounds(off, bytes.len())?;
        unsafe { p.write_bytes_at(off, &bytes) };
        Ok(recv.clone())
    }
    def "write_bytes" cfunc (recv, arg) {
        let bytes = str_bytes(arg)?;
        let p = ptr_of(recv);
        p.check_bounds(0, bytes.len())?;
        unsafe { p.write_bytes_at(0, &bytes) };
        Ok(recv.clone())
    }

    // -- typed arrays --
    // Four spellings per type, exactly as ruby-ffi's `AbstractMemory` has
    // them: `read_`/`write_` start at offset 0, `get_`/`put_` take one.
    def "read_array_of_int8" | "read_array_of_char"(r, count) { read_int_array(r, 0, count, 1, true) }
    def "read_array_of_uint8" | "read_array_of_uchar"(r, count) { read_int_array(r, 0, count, 1, false) }
    def "read_array_of_int16" | "read_array_of_short"(r, count) { read_int_array(r, 0, count, 2, true) }
    def "read_array_of_uint16" | "read_array_of_ushort"(r, count) { read_int_array(r, 0, count, 2, false) }
    def "read_array_of_int32" | "read_array_of_int"(r, count) { read_int_array(r, 0, count, 4, true) }
    def "read_array_of_uint32" | "read_array_of_uint"(r, count) { read_int_array(r, 0, count, 4, false) }
    def "read_array_of_int64" | "read_array_of_long" | "read_array_of_long_long"(r, count) { read_int_array(r, 0, count, 8, true) }
    def "read_array_of_uint64" | "read_array_of_ulong" | "read_array_of_ulong_long"(r, count) { read_int_array(r, 0, count, 8, false) }
    def "write_array_of_int8" | "write_array_of_char"(r, ary) { write_int_array(r, 0, ary, 1) }
    def "write_array_of_uint8" | "write_array_of_uchar"(r, ary) { write_int_array(r, 0, ary, 1) }
    def "write_array_of_int16" | "write_array_of_short"(r, ary) { write_int_array(r, 0, ary, 2) }
    def "write_array_of_uint16" | "write_array_of_ushort"(r, ary) { write_int_array(r, 0, ary, 2) }
    def "write_array_of_int32" | "write_array_of_int"(r, ary) { write_int_array(r, 0, ary, 4) }
    def "write_array_of_uint32" | "write_array_of_uint"(r, ary) { write_int_array(r, 0, ary, 4) }
    def "write_array_of_int64" | "write_array_of_long" | "write_array_of_long_long"(r, ary) { write_int_array(r, 0, ary, 8) }
    def "write_array_of_uint64" | "write_array_of_ulong" | "write_array_of_ulong_long"(r, ary) { write_int_array(r, 0, ary, 8) }
    def "read_array_of_double"(r, count) { read_float_array(r, 0, count, 8) }
    def "read_array_of_float"(r, count) { read_float_array(r, 0, count, 4) }
    def "write_array_of_double"(r, ary) { write_float_array(r, 0, ary, 8) }
    def "write_array_of_float"(r, ary) { write_float_array(r, 0, ary, 4) }
    def "read_array_of_pointer"(r, count) { read_pointer_array(r, 0, count) }
    def "write_array_of_pointer"(r, ary) { write_pointer_array(r, 0, ary) }
    def "read_array_of_string" cfunc (r, count?) { read_string_array(r, 0, count) }

    def "get_array_of_int8" | "get_array_of_char"(r, off, count) { read_int_array(r, off_arg(Some(off))?, count, 1, true) }
    def "get_array_of_uint8" | "get_array_of_uchar"(r, off, count) { read_int_array(r, off_arg(Some(off))?, count, 1, false) }
    def "get_array_of_int16" | "get_array_of_short"(r, off, count) { read_int_array(r, off_arg(Some(off))?, count, 2, true) }
    def "get_array_of_uint16" | "get_array_of_ushort"(r, off, count) { read_int_array(r, off_arg(Some(off))?, count, 2, false) }
    def "get_array_of_int32" | "get_array_of_int"(r, off, count) { read_int_array(r, off_arg(Some(off))?, count, 4, true) }
    def "get_array_of_uint32" | "get_array_of_uint"(r, off, count) { read_int_array(r, off_arg(Some(off))?, count, 4, false) }
    def "get_array_of_int64" | "get_array_of_long" | "get_array_of_long_long"(r, off, count) { read_int_array(r, off_arg(Some(off))?, count, 8, true) }
    def "get_array_of_uint64" | "get_array_of_ulong" | "get_array_of_ulong_long"(r, off, count) { read_int_array(r, off_arg(Some(off))?, count, 8, false) }
    def "put_array_of_int8" | "put_array_of_char"(r, off, ary) { write_int_array(r, off_arg(Some(off))?, ary, 1) }
    def "put_array_of_uint8" | "put_array_of_uchar"(r, off, ary) { write_int_array(r, off_arg(Some(off))?, ary, 1) }
    def "put_array_of_int16" | "put_array_of_short"(r, off, ary) { write_int_array(r, off_arg(Some(off))?, ary, 2) }
    def "put_array_of_uint16" | "put_array_of_ushort"(r, off, ary) { write_int_array(r, off_arg(Some(off))?, ary, 2) }
    def "put_array_of_int32" | "put_array_of_int"(r, off, ary) { write_int_array(r, off_arg(Some(off))?, ary, 4) }
    def "put_array_of_uint32" | "put_array_of_uint"(r, off, ary) { write_int_array(r, off_arg(Some(off))?, ary, 4) }
    def "put_array_of_int64" | "put_array_of_long" | "put_array_of_long_long"(r, off, ary) { write_int_array(r, off_arg(Some(off))?, ary, 8) }
    def "put_array_of_uint64" | "put_array_of_ulong" | "put_array_of_ulong_long"(r, off, ary) { write_int_array(r, off_arg(Some(off))?, ary, 8) }
    def "get_array_of_float64" | "get_array_of_double"(r, off, count) { read_float_array(r, off_arg(Some(off))?, count, 8) }
    def "get_array_of_float32" | "get_array_of_float"(r, off, count) { read_float_array(r, off_arg(Some(off))?, count, 4) }
    def "put_array_of_float64" | "put_array_of_double"(r, off, ary) { write_float_array(r, off_arg(Some(off))?, ary, 8) }
    def "put_array_of_float32" | "put_array_of_float"(r, off, ary) { write_float_array(r, off_arg(Some(off))?, ary, 4) }
    def "get_array_of_pointer"(r, off, count) { read_pointer_array(r, off_arg(Some(off))?, count) }
    def "put_array_of_pointer"(r, off, ary) { write_pointer_array(r, off_arg(Some(off))?, ary) }
    def "get_array_of_string" cfunc (r, off, count?) { read_string_array(r, off_arg(Some(off))?, count) }

    // `read_array_of_type(type, reader, length)` / its writer twin -- the
    // generic forms ruby-ffi's `pointer.rb` writes in Ruby, over the same
    // accessors above.
    def "read_array_of_type"(r, ty, reader, length) {
        let bytes = memptr_elem_size(ty)?;
        let name = method_name_arg(reader)?;
        read_typed_array(r, &name, bytes, length)
    }
    def "write_array_of_type"(r, ty, writer, ary) {
        let bytes = memptr_elem_size(ty)?;
        let name = method_name_arg(writer)?;
        write_typed_array(r, &name, bytes, ary)
    }

    // -- whole-buffer helpers --
    // `#clear` zeroes the whole extent; a pointer of unknown extent has
    // nothing to clear and ruby-ffi leaves it alone.
    def "clear"(recv) {
        let p = ptr_of(recv);
        if let Some(n) = p.size {
            p.check_bounds(0, n)?;
            for i in 0..n {
                unsafe { p.write_int(i, 1, 0) };
            }
        }
        Ok(recv.clone())
    }
    // `MemoryPointer#type_size` -- the element size `new` was given, so
    // `ptr[i]` and `#+ type_size` step one element.
    def "type_size"(recv) {
        Ok(RubyValue::Int(ptr_of(recv).type_size.load(Ordering::Relaxed) as i64))
    }
    // Byte order. zeo builds only for little-endian targets today, and
    // `order(:big)` would have to swap on every access -- refused loudly
    // rather than answered wrongly. The no-argument read is exact.
    def "order" cfunc (recv, want?) {
        let Some(want) = want else {
            return Ok(RubyValue::Symbol(Symbol::intern(
                if cfg!(target_endian = "little") { "little" } else { "big" },
            )));
        };
        let name = method_name_arg(want)?;
        let native = if cfg!(target_endian = "little") { "little" } else { "big" };
        if name == native || name == "native" {
            return Ok(recv.clone());
        }
        Err(crate::builtins::not_impl_error!("FFI::Pointer#order(:{name}) isn't supported yet (zeo limitation) -- \
                     this target is {native}-endian and no byte-swapping view exists"))
    }
    // `read_string_length(len)` -- exactly `len` bytes, NULs included, unlike
    // `read_string`'s NUL-terminated read.
    def "read_string_length"(recv, len) {
        let n = crate::ffi::to_i64(len)? as usize;
        let p = ptr_of(recv);
        p.check_bounds(0, n)?;
        Ok(bytes_to_str(unsafe { p.read_bytes_at(0, n) }))
    }

    // -- identity / arithmetic --
    def "null?"(recv) {
        Ok(RubyValue::Bool(ptr_of(recv).address() == 0))
    }
    // `#autorelease?` -- whether this pointer's memory goes away with the
    // object. True exactly when it OWNS that memory, so a `MemoryPointer`
    // answers true and a raw `Pointer` false, both oracle-verified. zeo
    // runs no finalizer over an owned buffer: Rust's own `Arc<OwnedBuf>`
    // drop is what actually frees it, at the same moment the last handle
    // goes, which is what the flag has always MEANT.
    def "autorelease?"(recv) {
        Ok(RubyValue::Bool(ptr_of(recv).autorelease.load(Ordering::Relaxed)))
    }
    def "autorelease="(recv, value) {
        let on = value.truthy();
        ptr_of(recv).autorelease.store(on, Ordering::Relaxed);
        Ok(RubyValue::Bool(on))
    }
    // `#free` is the gem's EAGER release. An owned buffer is reference
    // counted here, so the release happens when the last handle drops
    // rather than at this call -- a use-after-free is a crash in C and a
    // live read here, which is the safe direction to diverge in.
    def "free"(_recv) {
        Ok(RubyValue::Nil)
    }
    // Whether the extent is known (owned buffers yes, raw addresses no) --
    // fiddle sizes its wrappers off this.
    def "size_limit?"(recv) {
        Ok(RubyValue::Bool(ptr_of(recv).size.is_some()))
    }
    def "to_ptr"(recv) {
        Ok(recv.clone())
    }
    def "address" | "to_i"(recv) {
        Ok(RubyValue::Int(ptr_of(recv).address() as i64))
    }
    def "size" | "total"(recv) {
        Ok(RubyValue::Int(ptr_of(recv).size.unwrap_or(0) as i64))
    }
    def "+"(recv, other) {
        let delta = crate::ffi::to_i64(other)? as usize;
        Ok(RubyValue::Object(Arc::new(ptr_of(recv).offset(delta))))
    }
    // `eql?` is NOT an alias of this: CRuby's ffi gem defines only `==`, so a
    // pointer's `eql?` is `Object#eql?` -- identity. Measured, not assumed.
    def "=="(recv, other) {
        Ok(RubyValue::Bool(address_of(other) == Some(ptr_of(recv).address())))
    }
    def "slice"(recv, arg1, arg2) {
        let off = crate::ffi::to_i64(arg1)? as usize;
        let len = crate::ffi::to_i64(arg2)? as usize;
        let mut p = ptr_of(recv).offset(off);
        p.size = Some(len);
        Ok(RubyValue::Object(Arc::new(p)))
    }
}
