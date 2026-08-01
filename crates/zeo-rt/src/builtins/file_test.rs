//! `FileTest` (CRuby file.c) -- the 26 path predicates as a MIXIN.
//!
//! CRuby defines these functions once and exposes them twice: `File` gets them
//! as class methods, and `FileTest` gets them as `module_function`s, so
//! `include FileTest` makes `exist?(path)` callable without a receiver. Every
//! row here forwards to `File`'s class-method row of the same name rather than
//! restating the body, because two copies of `readable?` would drift.
//!
//! `FileTest` is deliberately NOT the whole of `File`'s class surface -- it
//! carries no `read`, `open` or `join`. It used to share `File`'s entire lookup
//! table, which made `FileTest.read` answer where CRuby raises NoMethodError.

use crate::builtins::file::file_test_forward;
use crate::{RubyValue, Signal};
use zeo_macros::ruby_module;

ruby_module! {
    FileTest = zeo_abi::FILE_TEST_MODULE;

    module_function def "exist?"(_recv, path) { one("exist?", path) }
    module_function def "file?"(_recv, path) { one("file?", path) }
    module_function def "directory?"(_recv, path) { one("directory?", path) }
    module_function def "size"(_recv, path) { one("size", path) }
    module_function def "size?"(_recv, path) { one("size?", path) }
    module_function def "zero?"(_recv, path) { one("zero?", path) }
    module_function def "empty?"(_recv, path) { one("empty?", path) }
    module_function def "readable?"(_recv, path) { one("readable?", path) }
    module_function def "readable_real?"(_recv, path) { one("readable_real?", path) }
    module_function def "writable?"(_recv, path) { one("writable?", path) }
    module_function def "writable_real?"(_recv, path) { one("writable_real?", path) }
    module_function def "executable?"(_recv, path) { one("executable?", path) }
    module_function def "executable_real?"(_recv, path) { one("executable_real?", path) }
    module_function def "owned?"(_recv, path) { one("owned?", path) }
    module_function def "grpowned?"(_recv, path) { one("grpowned?", path) }
    module_function def "pipe?"(_recv, path) { one("pipe?", path) }
    module_function def "socket?"(_recv, path) { one("socket?", path) }
    module_function def "blockdev?"(_recv, path) { one("blockdev?", path) }
    module_function def "chardev?"(_recv, path) { one("chardev?", path) }
    module_function def "setuid?"(_recv, path) { one("setuid?", path) }
    module_function def "setgid?"(_recv, path) { one("setgid?", path) }
    module_function def "sticky?"(_recv, path) { one("sticky?", path) }
    module_function def "symlink?"(_recv, path) { one("symlink?", path) }
    module_function def "world_readable?"(_recv, path) { one("world_readable?", path) }
    module_function def "world_writable?"(_recv, path) { one("world_writable?", path) }
    module_function def "identical?"(_recv, a, b) {
        file_test_forward("identical?", &[a.clone(), b.clone()])
    }
}

/// The one-path-argument case, which is 25 of the 26 rows.
fn one(name: &str, path: &RubyValue) -> Result<RubyValue, Signal> {
    file_test_forward(name, std::slice::from_ref(path))
}
