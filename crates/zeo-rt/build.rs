//! Derives `RUBY_PLATFORM` from the *build target* and exposes it as the
//! `ZEO_RUBY_PLATFORM` compile-time env, mirroring how CRuby's `configure`
//! bakes `RUBY_PLATFORM` (e.g. `arm64-darwin25`, `x86_64-linux`) from the host
//! at build time rather than probing at runtime. `bootstrap.rs` reads it via
//! `env!` to seed the `RUBY_PLATFORM`/`RUBY_DESCRIPTION` constants.

use std::process::Command;

fn main() {
    println!("cargo:rustc-env=ZEO_RUBY_PLATFORM={}", ruby_platform());
    println!("cargo:rerun-if-changed=build.rs");
}

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

/// The Darwin kernel major version of the build host (`uname -r` -> `25.5.0`
/// -> `25`), matching what CRuby's `configure` embeds. Empty if `uname` is
/// unavailable (never, on an Apple build host).
fn darwin_major() -> String {
    Command::new("uname")
        .arg("-r")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|r| r.trim().split('.').next().map(str::to_string))
        .unwrap_or_default()
}

fn cfg_var(key: &str) -> String {
    std::env::var(key).unwrap_or_default()
}
