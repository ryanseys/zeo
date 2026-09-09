//! `FileTest` (CRuby file.c) -- the 26 path predicates as a MIXIN.
//!
//! CRuby defines these functions once and exposes them twice: `File` gets them
//! as class methods, and `FileTest` gets them as `module_function`s, so
//! `include FileTest` makes `exist?(path)` callable without a receiver. Every
//! row here forwards to `File`'s class-method row of the same name rather than
//! restating the body, because two copies of `readable?` would drift.
//!
//! `FileTest` is deliberately NOT the whole of `File`'s class surface -- it
//! carries no `read`, `open` or `join`. Sharing `File`'s entire lookup table
//! would make `FileTest.read` answer where CRuby raises NoMethodError.

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Symbol;

    // Each test runs in its own nextest process -- see the crate README for
    // the with_core() bootstrap pattern.
    fn install_core() {
        crate::dispatch::install_class_registry(crate::dispatch::ClassRegistry::with_core());
    }

    fn ask(module: crate::ClassId, name: &str, path: &str) -> Result<RubyValue, crate::Signal> {
        crate::dispatch::send_value(
            &RubyValue::Class(module),
            Symbol::intern(name),
            &[RubyValue::Str(crate::string_new(path.to_string()))],
            None,
        )
    }

    #[test]
    fn the_forwarding_rows_answer_like_files_own() {
        install_core();
        // The test binary itself: a real file every run can see.
        let exe = std::env::current_exe().unwrap().display().to_string();
        for (name, path) in [
            ("exist?", "/"),
            ("exist?", "/definitely/not/here"),
            ("directory?", "/"),
            ("file?", exe.as_str()),
            ("file?", "/"),
        ] {
            let via_file_test = ask(zeo_abi::FILE_TEST_MODULE, name, path).unwrap();
            let via_file = ask(zeo_abi::FILE_CLASS, name, path).unwrap();
            let (RubyValue::Bool(a), RubyValue::Bool(b)) = (&via_file_test, &via_file) else {
                panic!("both answer booleans");
            };
            assert_eq!(a, b, "FileTest.{name}({path:?}) diverged from File's");
        }
    }

    #[test]
    fn file_test_is_not_the_whole_of_files_surface() {
        install_core();
        // `FileTest.read` must be the NoMethodError CRuby raises -- the module
        // does not share File's entire table.
        let err = ask(zeo_abi::FILE_TEST_MODULE, "read", "/etc/hosts");
        let Err(crate::Signal::Raise(exc)) = err else {
            panic!("expected a NoMethodError raise");
        };
        assert_eq!(
            exc.as_object_unchecked().class_id(),
            zeo_abi::NO_METHOD_ERROR_CLASS
        );
    }
}
