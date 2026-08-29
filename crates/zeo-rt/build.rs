//! Derives `RUBY_PLATFORM` from the *build target* and exposes it as the
//! `ZEO_RUBY_PLATFORM` compile-time env, mirroring how CRuby's `configure`
//! bakes `RUBY_PLATFORM` (e.g. `arm64-darwin25`, `x86_64-linux`) from the host
//! at build time rather than probing at runtime. `bootstrap.rs` reads it via
//! `env!` to seed the `RUBY_PLATFORM`/`RUBY_DESCRIPTION` constants.
//!
//! Also declares the extension modules, from the `ext/` tree itself -- see
//! [`declare_exts`].

use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rustc-env=ZEO_RUBY_PLATFORM={}", ruby_platform());
    println!("cargo:rerun-if-changed=build.rs");
    mirror_cext_layout();
    build_cext();
    declare_exts();
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
    if std::env::var_os("CARGO_FEATURE_CEXT").is_none() {
        return;
    }
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

/// Write `$OUT_DIR/ext_mods.rs`: one `mod` item per extension whose Rust this
/// build compiles, which `src/ext.rs` includes.
///
/// The list comes from the directory tree, so adding an extension is adding
/// its directory -- there is no list here to forget. An entry needs
/// `ext/<name>/src/lib.rs`, which is where a Rust-backed gem puts its Rust;
/// a directory with only a `lib/` (`fiddle`) is pure Ruby and declares no
/// module.
///
/// Cargo hands a build script the resolved feature set, so an extension the
/// build turned off is simply absent rather than emitted behind a `#[cfg]`.
/// Paths are absolute because `#[path]` inside an `include!` resolves against
/// the included file, not the including one.
/// One library's Rust half, at whatever path upstream puts its C.
///
/// Almost every gem is `ext/<name>/`, so `ext/<name>/ext/<name>/src/lib.rs`
/// is the usual answer. `io-console` is `ext/io/console/` upstream and keeps
/// that path here, so the fallback walks the `ext/` subtree for the one
/// `src/lib.rs` it holds.
fn native_half(ext: &Path, name: &str) -> Option<std::path::PathBuf> {
    let flat = ext.join(name).join("ext").join(name).join("src/lib.rs");
    if flat.is_file() {
        return Some(flat);
    }
    let mut found = None;
    let mut stack = vec![ext.join(name).join("ext")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).ok()?.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.ends_with("src/lib.rs") {
                assert!(
                    found.is_none(),
                    "{}/ holds more than one src/lib.rs -- one library, one \
                     native half",
                    ext.join(name).display()
                );
                found = Some(path);
            }
        }
    }
    found
}

fn declare_exts() {
    let root = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set by cargo");
    let ext = Path::new(&root).join("ext");
    println!("cargo:rerun-if-changed=ext");
    let mut names: Vec<String> = std::fs::read_dir(&ext)
        .unwrap_or_else(|e| panic!("reading {}: {e}", ext.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| native_half(&ext, n).is_some())
        .collect();
    names.sort();
    assert!(
        !names.is_empty(),
        "no extension has a Rust half under {} -- every `require` of one \
         would fail at run time",
        ext.display()
    );

    let mut code = String::from("// @generated by build.rs from the ext/ tree.\n");
    for name in &names {
        // A directory is named for the GEM, and a gem name may carry a hyphen
        // (`io-console`) where a Rust module and a cargo feature cannot.
        let ident = name.replace('-', "_");
        let feature = format!("CARGO_FEATURE_EXT_{}", ident.to_uppercase());
        if std::env::var_os(&feature).is_none() {
            continue;
        }
        let lib = native_half(&ext, name).expect("filtered above");
        println!("cargo:rerun-if-changed=ext/{name}/ext");
        code.push_str(&format!(
            "#[path = {:?}]\npub(crate) mod {ident};\n",
            lib.display().to_string()
        ));
    }
    let out = std::env::var("OUT_DIR").expect("OUT_DIR set by cargo");
    let dest = Path::new(&out).join("ext_mods.rs");
    if std::fs::read_to_string(&dest).is_ok_and(|old| old == code) {
        return;
    }
    std::fs::write(&dest, code).unwrap_or_else(|e| panic!("writing {}: {e}", dest.display()));
}

/// The pieces of the C extension surface that have to BE C.
///
/// `cext_jmp.c` holds the only `setjmp`: a `longjmp` past a live Rust frame
/// skips its destructors, which is undefined rather than merely leaky.
/// `cext_va.c`, `cext_fmt.c` and `cext_err.c` hold every variadic entry: Rust
/// cannot read a `va_list`, and guessing is how a pointer gets read out of the
/// wrong register. `cext_err.c` also owns `errno`, which is a macro over a
/// per-thread location Rust cannot name. `cext_native_thread.c` takes pointers
/// to a real `pthread_mutex_t` and `pthread_cond_t`, which is a layout only C
/// knows.
fn build_cext() {
    for f in CEXT_SOURCES {
        println!("cargo:rerun-if-changed=csrc/{f}");
    }
    if std::env::var_os("CARGO_FEATURE_CEXT").is_none() {
        return;
    }
    let mut build = cc::Build::new();
    for f in CEXT_SOURCES {
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

/// `<cpu>-<os>` in Ruby's spelling (its `RUBY_PLATFORM` convention), from the
/// Cargo target the crate is being built for.
fn ruby_platform() -> String {
    format!("{}-{}", ruby_arch(), ruby_os())
}

/// Ruby names a few CPUs differently from Rust's `target_arch` (notably
/// `aarch64` -> `arm64`); everything else passes through unchanged.
fn ruby_arch() -> String {
    match cfg_var("CARGO_CFG_TARGET_ARCH").as_str() {
        "aarch64" => "arm64".to_string(),
        "x86" => "i686".to_string(),
        other => other.to_string(),
    }
}

/// Ruby's OS token: `darwin<major>` (the Darwin kernel major, as `configure`
/// records it) on Apple targets; `linux`/`linux-musl` on Linux; the raw
/// `target_os` elsewhere.
fn ruby_os() -> String {
    match cfg_var("CARGO_CFG_TARGET_OS").as_str() {
        "macos" | "ios" | "tvos" | "watchos" => format!("darwin{}", darwin_major()),
        "linux" => match cfg_var("CARGO_CFG_TARGET_ENV").as_str() {
            "musl" => "linux-musl".to_string(),
            _ => "linux".to_string(),
        },
        other => other.to_string(),
    }
}

/// The Darwin kernel major version (`uname -r` -> `25.5.0` -> `25`), matching
/// what CRuby's `configure` embeds -- taken from the build host only when the
/// host IS a mac (native build). Cross-compiling to an Apple target from
/// elsewhere pins a contemporary default: the host kernel is meaningless for
/// the target, and the value is cosmetic (`RUBY_PLATFORM`'s suffix). Kept in
/// sync with the copy in `crates/zeo/build.rs` (`render_rbconfig`).
fn darwin_major() -> String {
    if std::env::consts::OS == "macos"
        && let Some(major) = Command::new("uname")
            .arg("-r")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|r| r.trim().split('.').next().map(str::to_string))
    {
        return major;
    }
    // ruby 4.0.6 era: Darwin 25 (macOS 26).
    "25".to_string()
}

fn cfg_var(key: &str) -> String {
    std::env::var(key).unwrap_or_default()
}
