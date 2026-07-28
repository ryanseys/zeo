//! `fcntl` -- the `Fcntl` module's `fcntl(2)`/`open(2)` flag constants, an
//! in-tree require-gated extension. `require "fcntl"` activates it.
//!
//! CRuby's `ext/fcntl` is a constant table and nothing else: the operations
//! themselves are `IO#fcntl`, which lives on IO. Every value here comes from
//! `libc` rather than being written down, because these are the platform's own
//! numbers -- `O_NOCTTY` is 0x20000 on macOS and 0x100 on Linux, and CRuby
//! reports whatever the header says. The names are `#ifdef`'d in CRuby for the
//! same reason and `#[cfg]`'d here.
//!
//! `VERSION` mirrors the bundled gem's version, as CRuby's does.

use crate::RubyValue;
use zeo_macros::ruby_module;

/// A flag as Ruby sees it. libc types these inconsistently -- `c_int` for the
/// commands, `c_short` for the lock types, `u32` for Darwin's `fstore_t` mode
/// bits -- so each call site widens to `i64` and this only wraps.
const fn flag(v: i64) -> RubyValue {
    RubyValue::Int(v)
}

ruby_module! {
    Fcntl = zeo_abi::FCNTL_MODULE;

    // The `fcntl(2)` commands.
    const F_DUPFD = flag(libc::F_DUPFD as i64);
    const F_GETFD = flag(libc::F_GETFD as i64);
    const F_SETFD = flag(libc::F_SETFD as i64);
    const F_GETFL = flag(libc::F_GETFL as i64);
    const F_SETFL = flag(libc::F_SETFL as i64);
    const F_GETLK = flag(libc::F_GETLK as i64);
    const F_SETLK = flag(libc::F_SETLK as i64);
    const F_SETLKW = flag(libc::F_SETLKW as i64);
    #[cfg(target_os = "linux")]
    const F_DUPFD_CLOEXEC = flag(libc::F_DUPFD_CLOEXEC as i64);

    // The one `F_SETFD` flag.
    const FD_CLOEXEC = flag(libc::FD_CLOEXEC as i64);

    // Lock types for the `F_*LK*` commands' `struct flock`.
    const F_RDLCK = flag(libc::F_RDLCK as i64);
    const F_WRLCK = flag(libc::F_WRLCK as i64);
    const F_UNLCK = flag(libc::F_UNLCK as i64);

    // `open(2)` flags -- the same set `File::Constants` carries, exposed here
    // too because that is where CRuby puts them.
    const O_CREAT = flag(libc::O_CREAT as i64);
    const O_EXCL = flag(libc::O_EXCL as i64);
    const O_TRUNC = flag(libc::O_TRUNC as i64);
    const O_APPEND = flag(libc::O_APPEND as i64);
    const O_NONBLOCK = flag(libc::O_NONBLOCK as i64);
    const O_NDELAY = flag(libc::O_NDELAY as i64);
    const O_NOCTTY = flag(libc::O_NOCTTY as i64);
    const O_RDONLY = flag(libc::O_RDONLY as i64);
    const O_WRONLY = flag(libc::O_WRONLY as i64);
    const O_RDWR = flag(libc::O_RDWR as i64);
    const O_ACCMODE = flag(libc::O_ACCMODE as i64);

    // Darwin's `F_PREALLOCATE` family: the command plus the `fstore_t` mode
    // and position-mode flags it takes. No Linux counterpart (`fallocate(2)`
    // is a syscall of its own there), so CRuby exports them only here.
    #[cfg(target_vendor = "apple")]
    const F_PREALLOCATE = flag(libc::F_PREALLOCATE as i64);
    #[cfg(target_vendor = "apple")]
    const F_ALLOCATECONTIG = flag(libc::F_ALLOCATECONTIG as i64);
    #[cfg(target_vendor = "apple")]
    const F_ALLOCATEALL = flag(libc::F_ALLOCATEALL as i64);
    #[cfg(target_vendor = "apple")]
    const F_ALLOCATEPERSIST = flag(0x8);
    #[cfg(target_vendor = "apple")]
    const F_PEOFPOSMODE = flag(libc::F_PEOFPOSMODE as i64);
    #[cfg(target_vendor = "apple")]
    const F_VOLPOSMODE = flag(libc::F_VOLPOSMODE as i64);

    const VERSION = RubyValue::Str(crate::string_new("1.3.0".to_string()));
}

#[cfg(test)]
mod tests {
    use crate::RubyValue;

    fn value(name: &str) -> Option<RubyValue> {
        crate::constants::const_get(zeo_abi::FCNTL_MODULE.0, name)
    }

    /// The portable half of the table, seeded and carrying POSIX's own numbers.
    /// The platform-specific names are asserted for PRESENCE only -- their
    /// values are whatever this target's headers say, which is the point.
    #[test]
    fn install_constants_seeds_the_table() {
        super::install_constants();

        for (name, want) in [
            ("F_DUPFD", 0),
            ("F_GETFD", 1),
            ("F_SETFD", 2),
            ("F_GETFL", 3),
            ("F_SETFL", 4),
            ("FD_CLOEXEC", 1),
            ("O_RDONLY", 0),
            ("O_WRONLY", 1),
            ("O_RDWR", 2),
            ("O_ACCMODE", 3),
        ] {
            assert!(
                matches!(value(name), Some(RubyValue::Int(v)) if v == want),
                "{name} should be {want}, got {:?}",
                value(name)
            );
        }

        for name in ["F_GETLK", "F_SETLK", "F_SETLKW", "F_RDLCK", "O_NOCTTY"] {
            assert!(
                matches!(value(name), Some(RubyValue::Int(_))),
                "{name} should be seeded"
            );
        }

        // `O_NDELAY` is the historical spelling and aliases `O_NONBLOCK`.
        assert!(matches!(
            (value("O_NDELAY"), value("O_NONBLOCK")),
            (Some(RubyValue::Int(a)), Some(RubyValue::Int(b))) if a == b
        ));
    }
}
