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

/// Every payload struct the views fill, with the entry that answers one.
const PAYLOADS: &[(&str, &str)] = &[
    ("RString", "rstring"),
    ("RArray", "rarray"),
    ("RObject", "robject"),
    ("RRegexp", "rregexp"),
    ("RMatch", "rmatch"),
    ("RFile", "rfile"),
    ("RData", "rdata"),
    ("RTypedData", "rtypeddata"),
];

/// The rule the patch series exists to enforce, in its two halves.
///
/// A payload struct keeps UPSTREAM'S LAYOUT, so an extension that reads one
/// -- date's `RTYPEDDATA(self)->data = dat`, strscan's `RREGEXP(re)->usecnt`
/// -- compiles. That is only sound because the cast macro stopped being a
/// cast: it calls an entry that answers a view the runtime owns. The moment
/// either half slips, the other becomes a read of bytes zeo does not own,
/// which is why one test watches both.
#[test]
fn every_payload_struct_is_upstream_and_reached_by_a_call() {
    let core = cext().join("include/ruby/internal/core");
    for (name, entry) in PAYLOADS {
        let file = core.join(format!("{}.h", name.to_lowercase()));
        let text = std::fs::read_to_string(&file).expect("the vendored header is present");
        assert!(
            text.contains(&format!("struct {name} {{")),
            "{name} has no definition in {}; an extension reading one of its \
             fields would fail to compile",
            file.display()
        );
        assert!(
            text.contains(&format!("rb_zeo_{entry}(RBIMPL_CAST((VALUE)(obj)))")),
            "{} does not reach its view through rb_zeo_{entry}",
            file.display()
        );
        assert!(
            !text.contains(&format!("RBIMPL_CAST((struct {name} *)")),
            "{}(obj) casts the object again; with `struct {name}` defined, \
             that reads a byte zeo does not own",
            name.to_uppercase()
        );
    }
}

/// `zeo.h` is where the rule is stated and every entry declared, so it is the
/// one file a reader has to find. A header that reaches a view without going
/// through it would be a second, undocumented door.
#[test]
fn every_view_entry_is_declared_in_one_place() {
    let text = std::fs::read_to_string(cext().join("include/ruby/internal/zeo.h"))
        .expect("zeo.h is present");
    for (name, entry) in PAYLOADS {
        assert!(
            text.contains(&format!("struct {name} *rb_zeo_{entry}(VALUE obj);")),
            "zeo.h does not declare rb_zeo_{entry}"
        );
    }
}
