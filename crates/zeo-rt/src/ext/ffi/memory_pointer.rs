//! `FFI::MemoryPointer < FFI::Pointer` -- a pointer that owns the heap buffer
//! it points at. It adds only the two constructors (`new`/`from_string`); every
//! read/write method is inherited from `FFI::Pointer` through the ancestry walk,
//! so this file registers a class table and no instance table.

use std::sync::Arc;

use super::{RPointer, memptr_elem_size, new_memory, str_bytes};
use crate::{RubyValue, Symbol};
use zeo_macros::ruby_class;

ruby_class! {
    MemoryPointer = zeo_abi::FFI_MEMORY_POINTER_CLASS < zeo_abi::FFI_POINTER_CLASS;

    // `MemoryPointer.new(type, count = 1, clear = true)`. `type` is a type
    // symbol (`:int` -> 4 bytes) or an Integer element size in bytes. A block
    // form yields the pointer and returns the block's value (the gem also
    // auto-frees afterward; our pointer is GC-managed, so the buffer simply
    // lives as long as it is referenced).
    def self."new" cfunc (_recv, arg1, arg2?, _arg3?, &block) {
        let elem = memptr_elem_size(arg1)?;
        let count = match arg2 {
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
    def self."from_string"(_recv, arg) {
        let bytes = str_bytes(arg)?;
        Ok(RubyValue::Object(Arc::new(RPointer::from_bytes_nul(&bytes))))
    }
}
