//! `FFI::Pointer < Object` -- the read/write surface over a raw C address
//! (`read_int`/`get_int`/`write_pointer`/`read_string`/`+`/`==`/...). Every
//! `MemoryPointer` inherits these through the ancestry walk; this file owns the
//! whole instance table plus `Pointer.new(address)`.

use std::os::raw::c_char;
use std::sync::Arc;

use super::{
    RPointer, address_of, bytes_to_str, off_arg, ptr_of, read_float_array, read_float_m,
    read_int_array, read_int_m, str_bytes, wrap_address, write_float_array, write_float_m,
    write_int_array, write_int_m,
};
use crate::builtins::{arity, index_error, type_error};
use crate::RubyValue;
use zeo_abi::FFI_POINTER_CLASS;
use zeo_macros::ruby_class;

ruby_class! {
    Pointer = zeo_abi::FFI_POINTER_CLASS < zeo_abi::OBJECT_CLASS;

    // `FFI::Pointer.new(address)` or `FFI::Pointer.new(type, address)` (the
    // type governs `[]` element size, which we don't model -- the address is
    // what matters). A Pointer argument copies its address.
    def self."new"(_recv, args, _b) {
        arity!(args, 1..=2);
        let addr_arg = args.last().expect("arity checked");
        let addr = match address_of(addr_arg) {
            Some(a) => a,
            None => crate::ffi::to_i64(addr_arg)? as usize,
        };
        Ok(RubyValue::Object(Arc::new(RPointer::raw(addr, FFI_POINTER_CLASS))))
    }

    // -- signed/unsigned integer reads (offset 0) --
    def "read_int8" | "read_char"(r, a, _b) { read_int_m(r, a, 1, true, false) }
    def "read_uint8" | "read_uchar"(r, a, _b) { read_int_m(r, a, 1, false, false) }
    def "read_int16" | "read_short"(r, a, _b) { read_int_m(r, a, 2, true, false) }
    def "read_uint16" | "read_ushort"(r, a, _b) { read_int_m(r, a, 2, false, false) }
    def "read_int32" | "read_int"(r, a, _b) { read_int_m(r, a, 4, true, false) }
    def "read_uint32" | "read_uint"(r, a, _b) { read_int_m(r, a, 4, false, false) }
    def "read_int64" | "read_long" | "read_long_long"(r, a, _b) { read_int_m(r, a, 8, true, false) }
    def "read_uint64" | "read_ulong" | "read_ulong_long"(r, a, _b) { read_int_m(r, a, 8, false, false) }

    // -- integer reads at an offset --
    def "get_int8" | "get_char"(r, a, _b) { read_int_m(r, a, 1, true, true) }
    def "get_uint8" | "get_uchar"(r, a, _b) { read_int_m(r, a, 1, false, true) }
    def "get_int16" | "get_short"(r, a, _b) { read_int_m(r, a, 2, true, true) }
    def "get_uint16" | "get_ushort"(r, a, _b) { read_int_m(r, a, 2, false, true) }
    def "get_int32" | "get_int"(r, a, _b) { read_int_m(r, a, 4, true, true) }
    def "get_uint32" | "get_uint"(r, a, _b) { read_int_m(r, a, 4, false, true) }
    def "get_int64" | "get_long" | "get_long_long"(r, a, _b) { read_int_m(r, a, 8, true, true) }
    def "get_uint64" | "get_ulong" | "get_ulong_long"(r, a, _b) { read_int_m(r, a, 8, false, true) }

    // -- integer writes (offset 0) --
    def "write_int8" | "write_char"(r, a, _b) { write_int_m(r, a, 1, false) }
    def "write_uint8" | "write_uchar"(r, a, _b) { write_int_m(r, a, 1, false) }
    def "write_int16" | "write_short"(r, a, _b) { write_int_m(r, a, 2, false) }
    def "write_uint16" | "write_ushort"(r, a, _b) { write_int_m(r, a, 2, false) }
    def "write_int32" | "write_int"(r, a, _b) { write_int_m(r, a, 4, false) }
    def "write_uint32" | "write_uint"(r, a, _b) { write_int_m(r, a, 4, false) }
    def "write_int64" | "write_long" | "write_long_long"(r, a, _b) { write_int_m(r, a, 8, false) }
    def "write_uint64" | "write_ulong" | "write_ulong_long"(r, a, _b) { write_int_m(r, a, 8, false) }

    // -- integer writes at an offset --
    def "put_int8" | "put_char"(r, a, _b) { write_int_m(r, a, 1, true) }
    def "put_uint8" | "put_uchar"(r, a, _b) { write_int_m(r, a, 1, true) }
    def "put_int16" | "put_short"(r, a, _b) { write_int_m(r, a, 2, true) }
    def "put_uint16" | "put_ushort"(r, a, _b) { write_int_m(r, a, 2, true) }
    def "put_int32" | "put_int"(r, a, _b) { write_int_m(r, a, 4, true) }
    def "put_uint32" | "put_uint"(r, a, _b) { write_int_m(r, a, 4, true) }
    def "put_int64" | "put_long" | "put_long_long"(r, a, _b) { write_int_m(r, a, 8, true) }
    def "put_uint64" | "put_ulong" | "put_ulong_long"(r, a, _b) { write_int_m(r, a, 8, true) }

    // -- floats --
    def "read_float"(r, a, _b) { read_float_m(r, a, 4, false) }
    def "read_double"(r, a, _b) { read_float_m(r, a, 8, false) }
    def "get_float32" | "get_float"(r, a, _b) { read_float_m(r, a, 4, true) }
    def "get_float64" | "get_double"(r, a, _b) { read_float_m(r, a, 8, true) }
    def "write_float"(r, a, _b) { write_float_m(r, a, 4, false) }
    def "write_double"(r, a, _b) { write_float_m(r, a, 8, false) }
    def "put_float32" | "put_float"(r, a, _b) { write_float_m(r, a, 4, true) }
    def "put_float64" | "put_double"(r, a, _b) { write_float_m(r, a, 8, true) }

    // -- pointers (read/write an address-sized word, wrapped as a Pointer) --
    def "read_pointer" | "get_pointer"(recv, args, _b) {
        arity!(args, 0..=1);
        let off = off_arg(args, 0)?;
        let p = ptr_of(recv);
        p.check_bounds(off, 8)?;
        Ok(wrap_address(unsafe { p.read_int(off, 8, false) } as usize))
    }
    def "write_pointer" | "put_pointer"(recv, args, _b) {
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
    def "read_string"(recv, args, _b) {
        arity!(args, 0..=1);
        let p = ptr_of(recv);
        match args.first() {
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
    def "get_string"(recv, args, _b) {
        arity!(args, 1..=2);
        let off = crate::ffi::to_i64(&args[0])? as usize;
        let p = ptr_of(recv);
        match args.get(1) {
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
    def "put_string"(recv, args, _b) {
        arity!(args, 2);
        let off = crate::ffi::to_i64(&args[0])? as usize;
        let bytes = str_bytes(&args[1])?;
        let p = ptr_of(recv);
        p.check_bounds(off, bytes.len() + 1)?;
        unsafe {
            p.write_bytes_at(off, &bytes);
            p.base.add(off + bytes.len()).write(0);
        }
        Ok(recv.clone())
    }
    // `read_bytes(len)` / `get_bytes(offset, len)` -- raw bytes, NUL-agnostic.
    def "read_bytes"(recv, args, _b) {
        arity!(args, 1);
        let n = crate::ffi::to_i64(&args[0])? as usize;
        let p = ptr_of(recv);
        p.check_bounds(0, n)?;
        Ok(bytes_to_str(unsafe { p.read_bytes_at(0, n) }))
    }
    def "get_bytes"(recv, args, _b) {
        arity!(args, 2);
        let off = crate::ffi::to_i64(&args[0])? as usize;
        let n = crate::ffi::to_i64(&args[1])? as usize;
        let p = ptr_of(recv);
        p.check_bounds(off, n)?;
        Ok(bytes_to_str(unsafe { p.read_bytes_at(off, n) }))
    }
    // `put_bytes(offset, str, index = 0, length = nil)` -- raw bytes (a slice
    // of `str` starting at `index`), no NUL.
    def "put_bytes"(recv, args, _b) {
        arity!(args, 2..=4);
        let off = crate::ffi::to_i64(&args[0])? as usize;
        let mut bytes = str_bytes(&args[1])?;
        let idx = match args.get(2) {
            None | Some(RubyValue::Nil) => 0,
            Some(v) => crate::ffi::to_i64(v)? as usize,
        };
        let len = match args.get(3) {
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
    def "write_bytes"(recv, args, _b) {
        arity!(args, 1);
        let bytes = str_bytes(&args[0])?;
        let p = ptr_of(recv);
        p.check_bounds(0, bytes.len())?;
        unsafe { p.write_bytes_at(0, &bytes) };
        Ok(recv.clone())
    }

    // -- typed arrays --
    def "read_array_of_int8"(r, a, _b) { read_int_array(r, a, 1, true) }
    def "read_array_of_uint8"(r, a, _b) { read_int_array(r, a, 1, false) }
    def "read_array_of_int16"(r, a, _b) { read_int_array(r, a, 2, true) }
    def "read_array_of_int32" | "read_array_of_int"(r, a, _b) { read_int_array(r, a, 4, true) }
    def "read_array_of_uint32" | "read_array_of_uint"(r, a, _b) { read_int_array(r, a, 4, false) }
    def "read_array_of_int64" | "read_array_of_long"(r, a, _b) { read_int_array(r, a, 8, true) }
    def "write_array_of_int8"(r, a, _b) { write_int_array(r, a, 1) }
    def "write_array_of_int16"(r, a, _b) { write_int_array(r, a, 2) }
    def "write_array_of_int32" | "write_array_of_int"(r, a, _b) { write_int_array(r, a, 4) }
    def "write_array_of_int64" | "write_array_of_long"(r, a, _b) { write_int_array(r, a, 8) }
    def "read_array_of_double"(r, a, _b) { read_float_array(r, a, 8) }
    def "read_array_of_float"(r, a, _b) { read_float_array(r, a, 4) }
    def "write_array_of_double"(r, a, _b) { write_float_array(r, a, 8) }
    def "write_array_of_float"(r, a, _b) { write_float_array(r, a, 4) }

    // -- identity / arithmetic --
    def "null?"(recv, args, _b) {
        arity!(args, 0);
        Ok(RubyValue::Bool(ptr_of(recv).address() == 0))
    }
    // Whether the extent is known (owned buffers yes, raw addresses no) --
    // fiddle sizes its wrappers off this.
    def "size_limit?"(recv, args, _b) {
        arity!(args, 0);
        Ok(RubyValue::Bool(ptr_of(recv).size.is_some()))
    }
    def "to_ptr"(recv, args, _b) {
        arity!(args, 0);
        Ok(recv.clone())
    }
    def "address" | "to_i"(recv, args, _b) {
        arity!(args, 0);
        Ok(RubyValue::Int(ptr_of(recv).address() as i64))
    }
    def "size" | "total"(recv, args, _b) {
        arity!(args, 0);
        Ok(RubyValue::Int(ptr_of(recv).size.unwrap_or(0) as i64))
    }
    def "+"(recv, args, _b) {
        arity!(args, 1);
        let delta = crate::ffi::to_i64(&args[0])? as usize;
        Ok(RubyValue::Object(Arc::new(ptr_of(recv).offset(delta))))
    }
    def "==" | "eql?"(recv, args, _b) {
        arity!(args, 1);
        Ok(RubyValue::Bool(address_of(&args[0]) == Some(ptr_of(recv).address())))
    }
    def "slice"(recv, args, _b) {
        arity!(args, 2);
        let off = crate::ffi::to_i64(&args[0])? as usize;
        let len = crate::ffi::to_i64(&args[1])? as usize;
        let mut p = ptr_of(recv).offset(off);
        p.size = Some(len);
        Ok(RubyValue::Object(Arc::new(p)))
    }
}
