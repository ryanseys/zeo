//! The vendored MRI headers still compile an ordinary C extension.
//!
//! `cext/patches/` rewrites every macro that reads object layout, and a
//! mistake there is not something a Ruby golden can see: it is a C compile
//! error in a gem nobody has built yet, or worse, a macro that still reads a
//! struct and answers a byte that means nothing. So the probe is C, and this
//! test is the thing that compiles it.
//!
//! `-fsyntax-only`: `rbimpl_zeo_*` has no body yet, and what is on trial is
//! the headers.

use std::path::PathBuf;
use std::process::Command;

fn cext() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("cext")
}

/// The compiler an extension would be built with. `CC` wins, as it does in
/// mkmf.
fn cc() -> String {
    std::env::var("CC").unwrap_or_else(|_| "cc".to_string())
}

#[test]
fn the_patched_headers_compile_a_c_extension() {
    let cext = cext();
    let out = Command::new(cc())
        .arg("-fsyntax-only")
        .arg("-Wall")
        .arg("-Wextra")
        .arg("-I")
        .arg(cext.join("config"))
        .arg("-I")
        .arg(cext.join("include"))
        .arg(cext.join("probe/layout.c"))
        .output()
        .expect("a C compiler is on PATH");

    let log = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "the patched headers no longer compile a plain C extension:\n{log}"
    );

    // A warning is not an error, but a NEW one is a patch that changed
    // meaning. Upstream warns under -Wextra on its own account, so only lines
    // naming zeo's own files count.
    let ours: Vec<&str> = log
        .lines()
        .filter(|l| l.contains(": warning:"))
        .filter(|l| l.contains("cext/probe/") || l.contains("internal/zeo.h"))
        .collect();
    assert!(
        ours.is_empty(),
        "zeo's own cext files warn:\n{}",
        ours.join("\n")
    );
}

/// The one rule the patch series exists to enforce: no macro an extension
/// calls is left dereferencing a payload struct. The structs are declared and
/// never defined, so a `RSTRING(v)->len` is a compile error rather than a
/// wrong byte -- this test proves they stayed incomplete.
#[test]
fn every_payload_struct_is_opaque() {
    let core = cext().join("include/ruby/internal/core");
    for name in [
        "RString",
        "RArray",
        "RRegexp",
        "RObject",
        "RMatch",
        "RData",
        "RTypedData",
    ] {
        let file = core.join(format!("{}.h", name.to_lowercase()));
        let text = std::fs::read_to_string(&file).expect("the vendored header is present");
        assert!(
            !text.contains(&format!("struct {name} {{")),
            "{name} is defined in {}; a `{}(v)->field` would read a byte zeo does not own",
            file.display(),
            name.to_uppercase()
        );
    }
}

/// `RFile` is the one payload struct with a definition, and it is safe only
/// because `RFILE(obj)` stopped being a cast: it calls `rb_zeo_rfile`, which
/// answers a view the runtime owns rather than the object's own bytes. The
/// moment that macro casts again, the definition becomes a wrong read.
#[test]
fn rfile_is_a_view_rather_than_a_cast() {
    let text = std::fs::read_to_string(cext().join("include/ruby/internal/core/rfile.h"))
        .expect("the vendored header is present");
    assert!(
        text.contains("struct RFile *rb_zeo_rfile(VALUE obj);"),
        "rfile.h no longer declares the view entry"
    );
    assert!(
        !text.contains("RBIMPL_CAST((struct RFile *)"),
        "RFILE casts the object again; with `struct RFile` now defined, \
         `RFILE(v)->fptr` would read a byte zeo does not own"
    );
}
