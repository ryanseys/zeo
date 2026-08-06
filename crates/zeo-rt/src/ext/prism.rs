//! The native half of the vendored `prism` gem -- private class methods on
//! `Prism` itself, which is where CRuby's C extension puts its own.
//!
//! Upstream ships two backends over one C library: a C extension (CRuby) and
//! an FFI one (every other engine). Both reduce to the same thing -- a
//! handful of `pm_serialize_*` calls that write a serialized buffer, which
//! `Prism::Serialize` then decodes into the node tree in pure Ruby. That is
//! the whole native surface, so these rows are that surface and nothing
//! more. The node classes, visitors and deserializer are the gem's own Ruby,
//! compiled like any other vendored gem.
//!
//! The library is the SAME prism zeo's own front end parses with (the
//! `ruby-prism` crate's C half), so a program never carries two copies.
//! The options argument is the byte string the gem's `dump_options` packs,
//! passed through untouched -- keeping the option encoding in one place,
//! upstream's.

use std::ffi::{CStr, c_char};

use crate::{RubyValue, Signal};
use zeo_macros::ruby_module;

/// Raw bytes as a BINARY String -- what `Prism::Serialize` requires of the
/// buffer it reads (it asserts the encoding).
fn bin_str(bytes: Vec<u8>) -> RubyValue {
    RubyValue::Str(crate::string_from_bytes(bytes, crate::encoding::ASCII_8BIT))
}

/// prism's growable output buffer. Allocated by size rather than by layout
/// (`pm_buffer_sizeof`) so a field reordering upstream cannot silently
/// corrupt it here.
struct Buffer(Vec<u8>);

unsafe extern "C" {
    fn pm_buffer_sizeof() -> usize;
    fn pm_buffer_init(buffer: *mut u8) -> bool;
    fn pm_buffer_value(buffer: *const u8) -> *mut c_char;
    fn pm_buffer_length(buffer: *const u8) -> usize;
    fn pm_buffer_free(buffer: *mut u8);
    fn pm_version() -> *const c_char;
    fn pm_serialize_parse(buffer: *mut u8, source: *const u8, size: usize, data: *const c_char);
    fn pm_serialize_lex(buffer: *mut u8, source: *const u8, size: usize, data: *const c_char);
    fn pm_serialize_parse_lex(buffer: *mut u8, source: *const u8, size: usize, data: *const c_char);
    fn pm_serialize_parse_comments(
        buffer: *mut u8,
        source: *const u8,
        size: usize,
        data: *const c_char,
    );
    fn pm_parse_success_p(source: *const u8, size: usize, data: *const c_char) -> bool;
}

impl Buffer {
    fn new() -> Option<Buffer> {
        let mut storage = vec![0u8; unsafe { pm_buffer_sizeof() }];
        unsafe { pm_buffer_init(storage.as_mut_ptr()) }.then_some(Buffer(storage))
    }

    fn as_ptr(&mut self) -> *mut u8 {
        self.0.as_mut_ptr()
    }

    /// The serialized bytes, copied out before the buffer is freed.
    fn take(&self) -> Vec<u8> {
        let ptr = unsafe { pm_buffer_value(self.0.as_ptr()) };
        let len = unsafe { pm_buffer_length(self.0.as_ptr()) };
        if ptr.is_null() {
            return Vec::new();
        }
        unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), len) }.to_vec()
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe { pm_buffer_free(self.0.as_mut_ptr()) };
    }
}

/// The `(source, options)` argument pair every entry point takes. Both are
/// byte strings: the source as written, and the packed options the gem's
/// own `dump_options` produced.
fn args_bytes(args: &[RubyValue]) -> Result<(Vec<u8>, Vec<u8>), Signal> {
    let source = crate::builtins::convert::to_rstr(&args[0])?
        .lock()
        .bytes()
        .to_vec();
    let mut options = crate::builtins::convert::to_rstr(&args[1])?
        .lock()
        .bytes()
        .to_vec();
    // `data` is read as a C string by prism even though its content is
    // binary-packed, so it must be NUL-terminated. The packed options never
    // end in a NUL of their own.
    options.push(0);
    Ok((source, options))
}

/// Runs one `pm_serialize_*` entry point over `args` and answers its buffer
/// as a binary string -- what `Prism::Serialize.load_*` reads.
fn serialize(
    args: &[RubyValue],
    f: unsafe extern "C" fn(*mut u8, *const u8, usize, *const c_char),
) -> Result<RubyValue, Signal> {
    let (source, options) = args_bytes(args)?;
    let Some(mut buffer) = Buffer::new() else {
        return Err(crate::dispatch::raise_error(
            "NoMemoryError",
            "failed to allocate a prism buffer".to_string(),
        ));
    };
    unsafe {
        f(
            buffer.as_ptr(),
            source.as_ptr(),
            source.len(),
            options.as_ptr().cast::<c_char>(),
        );
    }
    Ok(bin_str(buffer.take()))
}

ruby_module! {
    // The native entry points live on `Prism` ITSELF, as private class
    // methods. CRuby's prism puts its native half on `Prism` too (the C
    // extension defines `Prism.dump`/`lex`/... directly), so an extra
    // namespace would be a constant `Prism.constants` has and ruby's does
    // not. Private, because these are the backend's own seam -- the gem's
    // Ruby calls them with implicit self, and nothing outside should.
    Prism = zeo_abi::PRISM_MODULE;

    // The version of the linked prism, which the gem reports as
    // `Prism::VERSION`.
    private def self."version"(_recv) {
        let raw = unsafe { CStr::from_ptr(pm_version()) };
        Ok(RubyValue::Str(crate::string_new(raw.to_string_lossy().into_owned())))
    }

    // `Prism.dump`'s buffer: the serialized AST.
    private def self."serialize_parse"(_recv, _source, _options) {
        serialize(__args, pm_serialize_parse)
    }

    // `Prism.lex`'s buffer: the token stream with its lex states.
    private def self."serialize_lex"(_recv, _source, _options) {
        serialize(__args, pm_serialize_lex)
    }

    // `Prism.parse_lex`'s buffer: the AST and the token stream together.
    private def self."serialize_parse_lex"(_recv, _source, _options) {
        serialize(__args, pm_serialize_parse_lex)
    }

    // `Prism.parse_comments`' buffer.
    private def self."serialize_parse_comments"(_recv, _source, _options) {
        serialize(__args, pm_serialize_parse_comments)
    }

    // Whether the source parses with no errors -- answered without building or
    // serializing a tree. `native_` prefixed because this is the ONE native row
    // whose bare name collides with the public `Prism.parse_success?` the shim
    // defines over it; the `serialize_*` rows need no such guard.
    private def self."native_parse_success?"(_recv, _source, _options) {
        let (source, options) = args_bytes(__args)?;
        let ok = unsafe {
            pm_parse_success_p(
                source.as_ptr(),
                source.len(),
                options.as_ptr().cast::<c_char>(),
            )
        };
        Ok(RubyValue::Bool(ok))
    }
}
