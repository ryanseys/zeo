//! The three standard streams: the objects `$stdout`/`$stderr`/`$stdin`
//! start out holding, and what those globals answer after a reassignment.

use super::*;

pub fn stdout_value() -> RubyValue {
    static V: LazyLock<RubyValue> = LazyLock::new(|| std_io(StdStream::Stdout));
    V.clone()
}

pub fn stderr_value() -> RubyValue {
    static V: LazyLock<RubyValue> = LazyLock::new(|| std_io(StdStream::Stderr));
    V.clone()
}

pub fn stdin_value() -> RubyValue {
    static V: LazyLock<RubyValue> = LazyLock::new(|| std_io(StdStream::Stdin));
    V.clone()
}

/// Installs the `STDIN`/`STDOUT`/`STDERR` constants and the matching
/// `$stdin`/`$stdout`/`$stderr` globals -- called once from generated
/// `main()` (CRuby startup parity).
pub fn seed_stdio() {
    crate::constants::const_set(0, "STDOUT", stdout_value());
    crate::constants::const_set(0, "STDERR", stderr_value());
    crate::constants::const_set(0, "STDIN", stdin_value());
    crate::globals::seed_global(0, "$stdout", stdout_value());
    crate::globals::seed_global(0, "$stderr", stderr_value());
    crate::globals::seed_global(0, "$stdin", stdin_value());
    // `$>` is not a copy of `$stdout` but the SAME slot: assigning either
    // redirects both (oracle-verified in both directions). `PP.pp` defaults its
    // output to it, which is how a nil `$>` reached prettyprint as a receiver.
    crate::globals::global_alias(0, "$>", "$stdout");
}

/// The value `$stdout` currently holds in box 0 (nil -- never assigned --
/// means the default singleton). The print family targets this, so
/// `$stdout = STDERR` (or any duck-typed writer) redirects `puts`/`p`/....
/// Until some assignment has actually touched a stdio global
/// (`stdio_redirected`), the answer IS the seeded singleton -- returned
/// directly, skipping the alias-resolve and table locks per write call.
pub fn current_stdout() -> RubyValue {
    if !crate::globals::stdio_redirected() {
        return stdout_value();
    }
    match crate::globals::global_get(0, "$stdout") {
        RubyValue::Nil => stdout_value(),
        v => v,
    }
}

pub fn current_stderr() -> RubyValue {
    if !crate::globals::stdio_redirected() {
        return stderr_value();
    }
    match crate::globals::global_get(0, "$stderr") {
        RubyValue::Nil => stderr_value(),
        v => v,
    }
}
