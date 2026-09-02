//! Builds the two halves of the C API that are not Rust yet: the vendored
//! MRI headers' layout mirror, and the handful of C entries Rust cannot
//! write on the current toolchain.

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    mirror_cext_layout();
    build_cext();
}

/// Write `$OUT_DIR/cext_layout.rs`: the Rust mirror of every MRI struct the
/// C-extension views fill, generated from the vendored headers themselves.
///
/// The Rust that fills a view has to agree with the header to the byte, and a
/// hand-written `#[repr(C)]` copy would be a SECOND OWNER of that fact -- a
/// wrong offset in it reads or writes an unrelated field, silently. bindgen
/// derives the copy instead, and emits `offset_of` assertions with it, so a
/// header bump that moves a field fails THIS build by field name rather than
/// a gem's much later.
///
/// The allowlist is deliberately narrow: the structs the views fill, the
/// three shape flags zeo sets on an object so upstream's own accessors take
/// the heap arm, and the two flag enums the handle writes.
fn mirror_cext_layout() {
    println!("cargo:rerun-if-changed=cext/probe/mirror.h");
    println!("cargo:rerun-if-changed=cext/include");
    println!("cargo:rerun-if-changed=cext/config");
    let out = Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR")).join("cext_layout.rs");
    bindgen::Builder::default()
        .header("cext/probe/mirror.h")
        .clang_args(["-Icext/config", "-Icext/include"])
        .allowlist_type(MIRRORED)
        .generate()
        .expect("bindgen reads the vendored headers")
        .write_to_file(&out)
        .expect("writing cext_layout.rs");
}

/// One regex, because bindgen takes one. Every name here is either a struct a
/// view fills or an enum whose members zeo writes into an object's flags.
const MIRRORED: &str = "RBasic|RString|RArray|RObject|RRegexp|RMatch|RFile|RData|RTypedData\
|rb_io|rb_matchext_struct|rmatch_offset|re_registers|rb_data_type_struct\
|ruby_value_type|ruby_fl_type|ruby_rstring_flags|ruby_robject_flags|ruby_rarray_flags\
|rbimpl_typeddata_flags";

/// Compile `csrc/`: the entries that need C.
///
/// `cext_jmp.c` owns the `setjmp`/`longjmp` pair a raise travels through.
/// `cext_va.c`, `cext_fmt.c` and `cext_err.c` hold every variadic entry: Rust
/// cannot read a `va_list`, and guessing is how a pointer gets read out of the
/// wrong register. `cext_err.c` also owns `errno`, which is a macro over a
/// per-thread location Rust cannot name. `cext_native_thread.c` takes pointers
/// to a real `pthread_mutex_t` and `pthread_cond_t`, which is a layout only C
/// knows.
fn build_cext() {
    let mut build = cc::Build::new();
    for f in CEXT_SOURCES {
        println!("cargo:rerun-if-changed=csrc/{f}");
        build.file(format!("csrc/{f}"));
    }
    build
        .include("cext/include")
        .include("cext/config")
        .flag_if_supported("-Wno-unused-parameter");
    build.warnings(true).compile("zeo_cext");
}

const CEXT_SOURCES: &[&str] = &[
    "cext_jmp.c",
    "cext_va.c",
    "cext_fmt.c",
    "cext_err.c",
    "cext_native_thread.c",
];
