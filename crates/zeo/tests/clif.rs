//! The Cranelift backend's M0-16 gates: CLIF snapshots for a small
//! corpus, the capi-surface presence check against `libzeo.a`, and the
//! deterministic-output golden.

use std::path::PathBuf;

fn clif_of(source: &str) -> String {
    let text = zeo::compile_to_clif_text(source, &zeo::CompileOptions::default())
        .expect("the slice program must lower");
    // The default calling convention names the host (`apple_aarch64`,
    // `system_v`); normalize so the snapshots hold on every platform.
    text.replace("apple_aarch64", "ccall")
        .replace("system_v", "ccall")
}

#[test]
fn clif_snapshot_hello() {
    insta::assert_snapshot!(clif_of("puts \"Hello, world!\"\n"));
}

#[test]
fn clif_snapshot_fib() {
    insta::assert_snapshot!(clif_of(
        "def fib(n)\n  if n < 2\n    n\n  else\n    fib(n - 1) + fib(n - 2)\n  end\nend\nputs fib(10)\n",
    ));
}

#[test]
fn clif_snapshot_block_send() {
    insta::assert_snapshot!(clif_of(
        "total = 0\nq = Array.new(3, 2)\nq.each { |x| total = total + x }\nputs total\n",
    ));
}

/// A guarded fused `arr.each`: the receiver tag test, `iter_inline_ok_for`,
/// the inline arm and the dynamic fallback arm, all in one function.
///
/// This shape is why slot initialization moved to the entry block. The
/// block parameter's slot is created inside the INLINE arm, and the
/// epilogue releases every slot in `locals` unconditionally from both the
/// normal exit and the shared landing -- so a slot zeroed where it was
/// created is released uninitialised on the fallback path.
///
/// A golden cannot see this. Both arms print the right answer either way,
/// and every attempt to build a small failing program came out clean,
/// because the released bytes are stack garbage that happens to be benign
/// in a short frame. The snapshot is the proof: the zeroing stores belong
/// at the top of the entry block, ahead of the guards.
#[test]
fn clif_snapshot_fused_each_guards() {
    // The receiver must be a local analyze TYPES as an Array, or the block
    // compiles to an ordinary block fn and none of this appears.
    insta::assert_snapshot!(clif_of(
        "def each_of\n  a = [1, 2]\n  a.each { |x| p x }\nend\neach_of\n",
    ));
}

/// A send whose argument list runs before its literal block: the argument
/// is lowered, and only then does `zeo_rt_proc_new` build the proc.
///
/// The order is load-bearing, not cosmetic. The proc is MOVED to the
/// callee, so no landing can release it; an argument that raises between
/// the two leaks it, which is what
/// `a_raise_in_an_argument_keeps_the_block_unbuilt.rb` measures. It is
/// also ruby's own order -- the receiver and the arguments run first, and
/// the block is made last.
#[test]
fn clif_snapshot_block_after_arguments() {
    insta::assert_snapshot!(clif_of(
        "def take(n)\n  yield n\nend\ndef arg\n  1\nend\ndef go\n  take(arg) { |x| p x }\nend\ngo\n",
    ));
}

/// Two compiles of one program are byte-identical -- object emission is a
/// pure function of the CLIF.
#[test]
fn object_output_is_deterministic() {
    let src = "def add(a, b)\n  a + b\nend\nputs add(2, 3)\n";
    let a = zeo::compile_to_object_with(src, &zeo::CompileOptions::default(), false)
        .expect("compiles")
        .object;
    let b = zeo::compile_to_object_with(src, &zeo::CompileOptions::default(), false)
        .expect("compiles")
        .object;
    assert_eq!(a, b, "two compiles must produce identical object bytes");
}

/// Every symbol the emitter can import (`clif::capi_names::CAPI`) is a
/// defined `T` symbol in the runtime archive the link consumes.
#[test]
fn capi_surface_is_exported_by_the_archive() {
    let mut dir = std::env::current_exe().expect("test binary path");
    // target/<profile>/deps/<bin> -> target/<profile>
    dir.pop();
    dir.pop();
    let archive: PathBuf = dir.join("libzeo.a");
    assert!(
        archive.is_file(),
        "libzeo.a must sit beside the test profile dir: {}",
        archive.display()
    );
    let out = std::process::Command::new("nm")
        .arg(&archive)
        .output()
        .expect("nm must run");
    // macOS nm exits nonzero for archive members it cannot fully parse
    // (bitcode attribute drift, symbol-less members); the symbol listing
    // on stdout is still complete for the runtime's own objects, which is
    // all this asserts over.
    let nm = String::from_utf8_lossy(&out.stdout);
    assert!(
        !nm.is_empty(),
        "nm produced no listing for {}",
        archive.display()
    );
    let defined: std::collections::HashSet<&str> = nm
        .lines()
        .filter(|l| l.contains(" T "))
        .filter_map(|l| l.rsplit(' ').next())
        .map(|s| s.strip_prefix('_').unwrap_or(s))
        .collect();
    for row in zeo::clif::capi_names::CAPI {
        assert!(
            defined.contains(row.name),
            "capi_names row `{}` is not a defined symbol in libzeo.a",
            row.name
        );
    }
}

/// Every symbol the emitter can import resolves through the runtime's
/// in-process table (`zeo_rt::capi::symbols`) -- what the JIT run path
/// links against (the `zeo` binary does not export `zeo_rt_*`, so this
/// table IS the JIT's symbol source; a missing row would fail a user's
/// program at JIT relocation).
#[test]
fn capi_surface_resolves_in_process() {
    for row in zeo::clif::capi_names::CAPI {
        assert!(
            zeo::zeo_rt::capi::symbols::addr(row.name).is_some(),
            "capi_names row `{}` has no in-process address (add it to zeo-rt capi/symbols.rs)",
            row.name
        );
    }
}

/// The capi table itself is pinned -- an accidental signature change on
/// the emitter side shows up as a reviewed snapshot diff.
#[test]
fn capi_table_snapshot() {
    let mut rendered = String::new();
    for row in zeo::clif::capi_names::CAPI {
        rendered.push_str(&format!(
            "{} ({:?}) -> {:?}\n",
            row.name, row.params, row.ret
        ));
    }
    insta::assert_snapshot!(rendered);
}
