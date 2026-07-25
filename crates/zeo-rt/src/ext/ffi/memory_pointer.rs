//! `FFI::MemoryPointer < FFI::Pointer` -- a pointer that owns the heap buffer
//! it points at. It adds only the two constructors (`new`/`from_string`); every
//! read/write method is inherited from `FFI::Pointer` through the ancestry walk,
//! so this file registers a class table and no instance table.

use std::sync::Arc;

use super::{RPointer, memptr_elem_size, new_memory, str_bytes};
use crate::builtins::arity;
use crate::{RubyValue, Symbol};
use zeo_macros::ruby_class;

ruby_class! {
    MemoryPointer = zeo_abi::FFI_MEMORY_POINTER_CLASS < zeo_abi::FFI_POINTER_CLASS;

    // `MemoryPointer.new(type, count = 1, clear = true)`. `type` is a type
    // symbol (`:int` -> 4 bytes) or an Integer element size in bytes. A block
    // form yields the pointer and returns the block's value (the gem also
    // auto-frees afterward; our pointer is GC-managed, so the buffer simply
    // lives as long as it is referenced).
    def self."new"(_recv, args, block) {
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
    def self."from_string"(_recv, args, _b) {
        arity!(args, 1);
        let bytes = str_bytes(&args[0])?;
        Ok(RubyValue::Object(Arc::new(RPointer::from_bytes_nul(&bytes))))
    }
}
