//! The concurrency constructors compiled code builds directly. Everything
//! else on `Thread`/`Fiber`/`Ractor` reaches its ordinary builtin row
//! through dispatch; only `Ractor.new` needs a compile-time arm, because
//! its row deliberately refuses (the block has to be built as an isolated
//! Proc where the compiler can see the capture set).

use zeo_abi::abi::{STATUS_OK, STATUS_SIGNAL};

use crate::RubyValue;

/// `Ractor.new(*args, name: ...) { ... }`. `blk` is the already-built
/// Proc, MOVED in; `name` is the `name:` keyword's value (null = absent,
/// `nil` = unnamed as in CRuby); `loc` is the literal block's own
/// `file:line` for `#inspect` (empty = read the current frame, the shape
/// a dynamic proc takes).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_ractor_new(
    blk: *mut RubyValue,
    argv: *const RubyValue,
    argc: usize,
    name: *const RubyValue,
    loc: *const u8,
    loc_len: usize,
    out: *mut RubyValue,
) -> i32 {
    let block = if blk.is_null() {
        RubyValue::Nil
    } else {
        super::leakcheck::consumed(unsafe { &*blk });
        unsafe { std::ptr::read(blk) }
    };
    if !matches!(block, RubyValue::Proc(_)) {
        crate::signal::set_pending(crate::raise_error(
            "ArgumentError",
            "must be called with a block".to_string(),
        ));
        return STATUS_SIGNAL;
    }
    let args: Vec<RubyValue> = if argc == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(argv, argc) }.to_vec()
    };
    let name = match unsafe { name.as_ref() } {
        None | Some(RubyValue::Nil) => None,
        Some(v) => Some(v.clone()),
    };
    let loc = (loc_len > 0).then(|| unsafe { super::str_slice(loc, loc_len) }.to_string());
    match crate::ractor::ractor_new(block, args, name, loc) {
        Ok(v) => {
            super::leakcheck::created(&v);
            unsafe { out.write(v) };
            STATUS_OK
        }
        Err(sig) => {
            crate::signal::set_pending(sig);
            STATUS_SIGNAL
        }
    }
}
