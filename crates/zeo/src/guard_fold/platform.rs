//! Platform facts baked from the build target: `RUBY_PLATFORM`-derived
//! predicates, `RbConfig::CONFIG` keys, and the ffi gem's constants.

use super::*;

/// `Gem.win_platform?`'s build-time answer, derived from the baked
/// `RUBY_PLATFORM` the same way rubygems derives it at runtime. Shared with
/// the lower-stage class-body guard.
pub(crate) fn win_platform() -> Option<bool> {
    let platform = seeded_string_const("RUBY_PLATFORM")?;
    Some(
        ["mswin", "mingw", "cygwin"]
            .iter()
            .any(|w| platform.contains(w)),
    )
}

/// The ffi gem's `FFI::Platform.mac?`/`.windows?`/`.linux?`/`.unix?`/`.bsd?`
/// predicates, answered from the same baked `RUBY_PLATFORM` (smartcard picks
/// its `Word` typedef width by `FFI::Platform.mac?`). `unix?` is the gem's
/// own rule: everything that is not windows. Shared with the lower-stage
/// class-body guard so both stages pick one branch.
pub(crate) fn ffi_platform_predicate(name: &str) -> Option<bool> {
    let platform = seeded_string_const("RUBY_PLATFORM")?;
    let windows = ["mswin", "mingw", "cygwin"]
        .iter()
        .any(|w| platform.contains(w));
    Some(match name {
        "mac?" => platform.contains("darwin"),
        "windows?" => windows,
        "unix?" => !windows,
        "linux?" => platform.contains("linux"),
        // `IS_BSD = IS_MAC || IS_FREEBSD || ...` -- macOS counts, which is
        // what makes `bsd?` and `mac?` both true on darwin.
        "bsd?" => ["darwin", "freebsd", "openbsd", "netbsd", "dragonfly"]
            .iter()
            .any(|w| platform.contains(w)),
        "solaris?" => platform.contains("solaris"),
        _ => return None,
    })
}

/// A `RbConfig::CONFIG[key]` entry this build bakes, or `None` for a key whose
/// value zeo doesn't decide. These are the SAME strings the generated program's
/// `RbConfig::CONFIG` answers (both come from `build.rs`), so folding a
/// `RbConfig::CONFIG['host_os'] =~ /linux/` guard picks the branch the program
/// itself would run. `host_os` is the older spelling of the platform question
/// `RUBY_PLATFORM` also answers -- sys-uname gates its `utsname` layout on it.
pub(crate) fn rbconfig_string(key: &str) -> Option<&'static str> {
    Some(match key {
        "host_os" => env!("ZEO_HOST_OS"),
        "host_cpu" => env!("ZEO_HOST_CPU"),
        "arch" | "sitearch" => env!("ZEO_RUBY_PLATFORM"),
        "DLEXT" => env!("ZEO_DLEXT"),
        "SOEXT" => env!("ZEO_SOEXT"),
        _ => return None,
    })
}

/// `FFI::Platform::ARCH` / `::OS` / `::NAME` as the ffi gem derives them
/// from the host triple (audio picks `CFIndex`'s width by
/// `FFI::Platform::ARCH == 'x86_64'`). The gem spells arm64 as `aarch64`.
pub(crate) fn ffi_platform_string(leaf: &str) -> Option<String> {
    let platform = seeded_string_const("RUBY_PLATFORM")?;
    let os = if platform.contains("darwin") {
        "darwin"
    } else if ["mswin", "mingw", "cygwin"]
        .iter()
        .any(|w| platform.contains(w))
    {
        "windows"
    } else if platform.contains("linux") {
        "linux"
    } else {
        return None;
    };
    let arch = match platform.split('-').next()? {
        "arm64" | "aarch64" => "aarch64",
        other => other,
    };
    Some(match leaf {
        "ARCH" => arch.to_string(),
        "OS" => os.to_string(),
        "NAME" => format!("{arch}-{os}"),
        _ => return None,
    })
}

/// The ffi gem's `FFI::Platform` NUMERIC constants, in the same bits the
/// runtime half reads back off `FFI::Type::Builtin`
/// (`crates/zeo-rt/ext/ffi/lib/ffi.rb`) --
/// the two have to agree, or a `typedef` folded one way at compile time
/// contradicts the constant the program prints. vips gates its `:GType`
/// width on `ADDRESS_SIZE == 64` and crabstone its `:size_t` on
/// `ADDRESS_SIZE == 32`.
pub(crate) fn ffi_platform_integer(leaf: &str) -> Option<i64> {
    // `long double` is 16 bytes everywhere zeo emits -- 80-bit extended on
    // x86_64, IEEE quad on aarch64 Linux -- with ONE exception: Apple aliases
    // it to `double` on arm64 only. Testing `darwin` alone answered 64 on an
    // Intel Mac, where CRuby's ffi says 128.
    let platform = seeded_string_const("RUBY_PLATFORM")?;
    let long_double = match platform.contains("darwin") && platform.starts_with("arm64") {
        true => 64,
        false => 128,
    };
    Some(match leaf {
        "ADDRESS_SIZE" | "ADDRESS_ALIGN" => i64::from(usize::BITS),
        "LONG_SIZE" | "LONG_ALIGN" | "INT64_SIZE" | "INT64_ALIGN" | "DOUBLE_SIZE"
        | "DOUBLE_ALIGN" => 64,
        "INT32_SIZE" | "INT32_ALIGN" | "FLOAT_SIZE" | "FLOAT_ALIGN" => 32,
        "INT16_SIZE" | "INT16_ALIGN" => 16,
        "INT8_SIZE" | "INT8_ALIGN" => 8,
        "LONG_DOUBLE_SIZE" | "LONG_DOUBLE_ALIGN" => long_double,
        // zeo emits for little-endian targets only.
        "LITTLE_ENDIAN" | "BYTE_ORDER" => 1234,
        "BIG_ENDIAN" => 4321,
        _ => return None,
    })
}

/// [`ffi_platform_string`] addressed by a whole constant path, for the
/// analyze-stage folds -- which reach the constant only when the program
/// required `ffi` at all, and see an initializer they cannot fold when it did
/// (`ADDRESS_SIZE` reads its bits back off an `FFI::Type` instance).
pub(super) fn ffi_platform_leaf(path: &str) -> Option<String> {
    ffi_platform_string(
        path.trim_start_matches("::")
            .strip_prefix("FFI::Platform::")?,
    )
}

/// [`ffi_platform_integer`]'s constant-path spelling, the twin of
/// [`ffi_platform_leaf`].
pub(super) fn ffi_platform_leaf_integer(path: &str) -> Option<i64> {
    ffi_platform_integer(
        path.trim_start_matches("::")
            .strip_prefix("FFI::Platform::")?,
    )
}
