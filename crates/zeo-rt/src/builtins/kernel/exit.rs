//! `Kernel`'s process-exit surface -- `exit`, `exit!`, `abort`, and the
//! `SystemExit` plumbing `raise` shares. The `ruby_module!` rows stay in
//! `mod.rs` and call these by bare name.

use super::*;

/// `Kernel#exit(status = true)` -- raises a RESCUABLE `SystemExit` carrying the
/// status, exactly as CRuby does: it unwinds through `ensure` blocks and can be
/// caught by `rescue SystemExit`. Only if it reaches the top level uncaught does
/// the process actually exit (see the generated `main`'s handler, which runs
/// `at_exit` first).
pub fn kernel_exit(args: &[RubyValue]) -> crate::Signal {
    let status = match args.first() {
        None | Some(RubyValue::Bool(true)) => 0,
        Some(RubyValue::Bool(false)) => 1,
        Some(RubyValue::Int(n)) => *n,
        Some(_) => 0,
    };
    crate::dispatch::raise_error_details(
        "SystemExit",
        "exit".to_string(),
        &[("status", RubyValue::Int(status))],
    )
}

/// `Kernel#abort(message = nil)` -- writes `message` to stderr IMMEDIATELY
/// (CRuby's own order, so it appears even when the SystemExit is rescued), then
/// raises `SystemExit` with status 1 and that message.
pub fn kernel_abort(args: &[RubyValue]) -> crate::Signal {
    let msg = match args.first() {
        Some(v) => {
            let s = v.to_display_string();
            eprintln!("{s}");
            s
        }
        None => "exit".to_string(),
    };
    crate::dispatch::raise_error_details("SystemExit", msg, &[("status", RubyValue::Int(1))])
}

/// `Kernel#exit!(status = false)` -- CRuby's uncatchable immediate exit: no
/// `SystemExit`, no `ensure`, no `at_exit`.
pub fn kernel_exit_bang(args: &[RubyValue]) -> ! {
    let code = match args.first() {
        None | Some(RubyValue::Bool(false)) => 1,
        Some(RubyValue::Bool(true)) => 0,
        Some(RubyValue::Int(n)) => *n as i32,
        Some(_) => 1,
    };
    // CRuby's `exit!` is `_exit(2)`: it runs no `at_exit` handler and
    // DISCARDS whatever stdio still holds (`print "x"; exit!` writes
    // nothing, where `exit` writes the `x`). `std::process::exit` runs
    // Rust's own cleanup, which flushes -- so the buffer has to be
    // stepped around, not asked politely.
    unsafe { libc::_exit(code) }
}

/// The exit status carried by `exc` when it IS a `SystemExit`, else `None` --
/// what the generated top-level consults to exit quietly with that status
/// instead of reporting an uncaught exception.
pub fn system_exit_status(exc: &RubyValue) -> Option<i32> {
    let RubyValue::Object(o) = exc else {
        return None;
    };
    if !crate::dispatch::is_a(o.class_id(), zeo_abi::SYSTEM_EXIT_CLASS) {
        return None;
    }
    // Read the status through its own `#status` row rather than a private
    // detail accessor, so the two can't drift.
    Some(
        match crate::dispatch::send_value(exc, crate::Symbol::intern("status"), &[], None) {
            Ok(RubyValue::Int(n)) => n as i32,
            _ => 0,
        },
    )
}

/// The exception `raise`'s argument list names -- everything the row does
/// before it signals. Shared with the explicit-`cause:` entry, which needs
/// the built exception in hand before it raises.
pub(crate) fn build_raise_exception(args: &[RubyValue]) -> Result<RubyValue, Signal> {
    match args {
        // Bare re-raise: the exception being rescued, exactly (same
        // object); outside any rescue, a fresh EMPTY-message
        // RuntimeError (oracle-verified, matching `emit_raise`).
        [] => match crate::current_exception() {
            Some(e) => Ok(e),
            None => {
                crate::dispatch::coerce_raise_arg(RubyValue::Str(crate::string_new(String::new())))
            }
        },
        [first, rest @ ..] => {
            // At most the message reaches `exception`; `rest[1]` is the
            // dropped backtrace.
            let msg = &rest[..rest.len().min(1)];
            match first {
                RubyValue::Class(cid) => {
                    if !crate::dispatch::is_a(*cid, zeo_abi::EXCEPTION_CLASS) {
                        return Err(type_error!("exception class/object expected"));
                    }
                    crate::dispatch::send_value(first, Symbol::intern("exception"), msg, None)
                }
                RubyValue::Object(o)
                    if crate::dispatch::is_a(o.class_id(), zeo_abi::EXCEPTION_CLASS) =>
                {
                    if msg.is_empty() {
                        Ok(first.clone())
                    } else {
                        crate::dispatch::send_value(first, Symbol::intern("exception"), msg, None)
                    }
                }
                RubyValue::Str(_) if rest.is_empty() => {
                    crate::dispatch::coerce_raise_arg(first.clone())
                }
                // Anything else goes through CRuby's `rb_make_exception`
                // protocol: an object whose class answers `#exception` may
                // be raised, and only what that refuses is the TypeError.
                // Refusing here outright made `raise WithHook.new, "hi"`
                // a TypeError where the emitter's own folded site ran the
                // hook.
                _ if msg.is_empty() => crate::dispatch::coerce_raise_arg(first.clone()),
                _ => crate::dispatch::coerce_raise_arg_with_message(first.clone(), msg[0].clone()),
            }
        }
    }
}
