//! Derives `RUBY_PLATFORM` from the *build target* and exposes it as the
//! `ZEO_RUBY_PLATFORM` compile-time env, mirroring how CRuby's `configure`
//! bakes `RUBY_PLATFORM` (e.g. `arm64-darwin25`, `x86_64-linux`) from the host
//! at build time rather than probing at runtime. `bootstrap.rs` reads it via
//! `env!` to seed the `RUBY_PLATFORM`/`RUBY_DESCRIPTION` constants.

use std::process::Command;

fn main() {
    println!("cargo:rustc-env=ZEO_RUBY_PLATFORM={}", ruby_platform());
    println!("cargo:rerun-if-changed=build.rs");
    build_cext();
}

/// The pieces of the C extension surface that have to BE C.
///
/// `cext_jmp.c` holds the only `setjmp`: a `longjmp` past a live Rust frame
/// skips its destructors, which is undefined rather than merely leaky.
/// `cext_va.c`, `cext_fmt.c` and `cext_err.c` hold every variadic entry: Rust
/// cannot read a `va_list`, and guessing is how a pointer gets read out of the
/// wrong register. `cext_err.c` also owns `errno`, which is a macro over a
/// per-thread location Rust cannot name.
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
    build.warnings(true).compile("zeo_cext");
}

const CEXT_SOURCES: &[&str] = &["cext_jmp.c", "cext_va.c", "cext_fmt.c", "cext_err.c"];

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
