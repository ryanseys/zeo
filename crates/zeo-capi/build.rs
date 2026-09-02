//! Builds the handful of C entries Rust cannot write on the current
//! toolchain.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    build_cext();
}

/// Compile `csrc/`: the variadic entries, which Rust cannot write before
/// 1.99 -- it cannot read a `va_list`, and guessing is how a pointer gets
/// read out of the wrong register.
///
/// A raise is a Rust unwind through these frames (they call the Rust
/// entries that raise), so they carry unwind tables, exactly as a gem's own
/// objects must.
fn build_cext() {
    let mut build = cc::Build::new();
    for f in CEXT_SOURCES {
        println!("cargo:rerun-if-changed=csrc/{f}");
        build.file(format!("csrc/{f}"));
    }
    build
        .include("cext/include")
        .include("cext/config")
        .flag("-fexceptions")
        .flag("-fasynchronous-unwind-tables")
        .flag_if_supported("-Wno-unused-parameter")
        // Nothing in Rust calls these; an extension resolves them at load.
        // Without this the linker drops every member of the archive.
        .link_lib_modifier("+whole-archive");
    build.warnings(true).compile("zeo_cext");
}

const CEXT_SOURCES: &[&str] = &["cext_va.c", "cext_fmt.c", "cext_err.c"];
