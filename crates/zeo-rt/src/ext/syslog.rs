//! `syslog` -- the `Syslog` module over the system `syslog(3)` facility, an
//! in-tree require-gated extension. One process-wide connection, CRuby's own
//! model: `open` raises if already open, the accessors answer nil while
//! closed, and `log` formats in Ruby-land (`sprintf`) then hands the result
//! through a literal `"%s"` so a `%` in the MESSAGE can never reach the C
//! library as a format directive.
//!
//! The `Constants`/`Level`/`Option`/`Facility`/`Macros` submodules are the
//! gem's Ruby half (`gems/syslog/lib/syslog.rb`); every value they publish
//! reads back off this module's own constant table, so the two halves cannot
//! drift.

use crate::builtins::{arg_error, arity, convert, runtime_error};
use crate::{RubyValue, Signal};
use zeo_macros::ruby_module;

/// The one connection's state. `openlog(3)` KEEPS the ident pointer for the
/// connection's lifetime rather than copying it, which is why the `CString`
/// lives here (and why CRuby's extension stores its ident the same way).
struct LogState {
    ident: Option<std::ffi::CString>,
    options: i64,
    facility: i64,
}

static STATE: parking_lot::Mutex<LogState> = parking_lot::Mutex::new(LogState {
    ident: None,
    options: 0,
    facility: 0,
});

fn syslog_value() -> RubyValue {
    RubyValue::Class(zeo_abi::SYSLOG_MODULE)
}

/// The ident argument, or CRuby's default: `$0`, the program name.
fn ident_arg(v: Option<&RubyValue>) -> Result<String, Signal> {
    match v {
        None | Some(RubyValue::Nil) => Ok(crate::globals::global_get(0, "$0").to_display_string()),
        Some(other) => Ok(convert::to_rstr(other)?.lock().to_utf8_lossy().into_owned()),
    }
}

fn int_arg(v: Option<&RubyValue>, default: i64) -> Result<i64, Signal> {
    match v {
        None | Some(RubyValue::Nil) => Ok(default),
        Some(other) => convert::to_index(other),
    }
}

/// `openlog(3)` with the state updated to match; the caller has verified the
/// connection is closed. Mask resets to "everything" (0xff) on each open,
/// CRuby's observed behaviour.
fn do_open(ident: String, options: i64, facility: i64) -> Result<(), Signal> {
    let c_ident =
        std::ffi::CString::new(ident).map_err(|_| arg_error!("string contains null byte"))?;
    let mut st = STATE.lock();
    // SAFETY: the CString is stored in STATE below and outlives the
    // connection, satisfying openlog's keep-the-pointer contract.
    unsafe {
        libc::openlog(
            c_ident.as_ptr(),
            options as libc::c_int,
            facility as libc::c_int,
        );
        libc::setlogmask(0xff);
    }
    *st = LogState {
        ident: Some(c_ident),
        options,
        facility,
    };
    Ok(())
}

/// The shared engine of `log` and the per-priority shortcuts: sprintf the
/// format against its arguments, then emit the RESULT through a literal
/// `"%s"` -- message bytes never become format directives (CRuby does the
/// same, and it is the whole defense against `log(pri, user_string)`).
fn log_with(pri: i64, args: &[RubyValue]) -> Result<RubyValue, Signal> {
    if STATE.lock().ident.is_none() {
        return Err(runtime_error!("must open syslog before write"));
    }
    let fmt = convert::to_rstr(&args[0])?
        .lock()
        .to_utf8_lossy()
        .into_owned();
    let msg = crate::builtins::format::sprintf(&fmt, &args[1..])?;
    let c_msg = std::ffi::CString::new(msg).map_err(|_| arg_error!("string contains null byte"))?;
    // SAFETY: both strings are live NUL-terminated buffers for the call.
    unsafe { libc::syslog(pri as libc::c_int, c"%s".as_ptr(), c_msg.as_ptr()) };
    Ok(syslog_value())
}

/// CRuby's ArgumentError shape for a "this many or more" arity, which the
/// `arity!` macro's fixed/closed-range forms don't cover.
fn min_arity(args: &[RubyValue], min: usize) -> Result<(), Signal> {
    if args.len() < min {
        return Err(arg_error!(
            "wrong number of arguments (given {}, expected {min}+)",
            args.len()
        ));
    }
    Ok(())
}

const fn level(v: libc::c_int) -> RubyValue {
    RubyValue::Int(v as i64)
}

ruby_module! {
    Syslog = zeo_abi::SYSLOG_MODULE;

    // Severity levels, `syslog(3)`'s own ordering.
    const LOG_EMERG = level(libc::LOG_EMERG);
    const LOG_ALERT = level(libc::LOG_ALERT);
    const LOG_CRIT = level(libc::LOG_CRIT);
    const LOG_ERR = level(libc::LOG_ERR);
    const LOG_WARNING = level(libc::LOG_WARNING);
    const LOG_NOTICE = level(libc::LOG_NOTICE);
    const LOG_INFO = level(libc::LOG_INFO);
    const LOG_DEBUG = level(libc::LOG_DEBUG);

    // `openlog(3)` options.
    const LOG_PID = level(libc::LOG_PID);
    const LOG_CONS = level(libc::LOG_CONS);
    const LOG_ODELAY = level(libc::LOG_ODELAY);
    const LOG_NDELAY = level(libc::LOG_NDELAY);
    const LOG_NOWAIT = level(libc::LOG_NOWAIT);
    const LOG_PERROR = level(libc::LOG_PERROR);

    // Facilities -- the set CRuby's `#ifdef`s expose on the Unixes zeo
    // targets (both platforms carry all of these).
    const LOG_KERN = level(libc::LOG_KERN);
    const LOG_USER = level(libc::LOG_USER);
    const LOG_MAIL = level(libc::LOG_MAIL);
    const LOG_DAEMON = level(libc::LOG_DAEMON);
    const LOG_AUTH = level(libc::LOG_AUTH);
    const LOG_SYSLOG = level(libc::LOG_SYSLOG);
    const LOG_LPR = level(libc::LOG_LPR);
    const LOG_NEWS = level(libc::LOG_NEWS);
    const LOG_UUCP = level(libc::LOG_UUCP);
    const LOG_CRON = level(libc::LOG_CRON);
    const LOG_AUTHPRIV = level(libc::LOG_AUTHPRIV);
    const LOG_FTP = level(libc::LOG_FTP);
    const LOG_LOCAL0 = level(libc::LOG_LOCAL0);
    const LOG_LOCAL1 = level(libc::LOG_LOCAL1);
    const LOG_LOCAL2 = level(libc::LOG_LOCAL2);
    const LOG_LOCAL3 = level(libc::LOG_LOCAL3);
    const LOG_LOCAL4 = level(libc::LOG_LOCAL4);
    const LOG_LOCAL5 = level(libc::LOG_LOCAL5);
    const LOG_LOCAL6 = level(libc::LOG_LOCAL6);
    const LOG_LOCAL7 = level(libc::LOG_LOCAL7);

    // The bundled gem's version, as CRuby reports it.
    const VERSION = RubyValue::Str(crate::string_new("0.4.0".to_string()));

    // `Syslog.open(ident = $0, options = LOG_PID | LOG_CONS, facility =
    // LOG_USER)` -- open the process's one connection, answering the module
    // (or yielding it and closing after, the block form).
    def self."open" arity -1 (_recv, *args, &block) {
        arity!(args, 0..=3);
        if STATE.lock().ident.is_some() {
            return Err(runtime_error!("syslog already open"));
        }
        let ident = ident_arg(args.first())?;
        let options = int_arg(args.get(1), (libc::LOG_PID | libc::LOG_CONS) as i64)?;
        let facility = int_arg(args.get(2), libc::LOG_USER as i64)?;
        do_open(ident, options, facility)?;
        let Some(RubyValue::Proc(p)) = block else {
            return Ok(syslog_value());
        };
        let out = p.call(&[syslog_value()]);
        let mut st = STATE.lock();
        // SAFETY: plain closelog; the ident CString drops after it.
        unsafe { libc::closelog() };
        st.ident = None;
        out.map(|_| syslog_value())
    }

    // `reopen`/`open!` -- close (raising if there is nothing open, as the
    // plain `close` would) and open again with the NEW arguments; omitted
    // ones fall back to the defaults, not to the previous values.
    def self."reopen" | "open!" arity -1 (recv, *args, &block) {
        {
            let mut st = STATE.lock();
            if st.ident.is_none() {
                return Err(runtime_error!("syslog not opened"));
            }
            // SAFETY: plain closelog; the kept ident drops with the state swap.
            unsafe { libc::closelog() };
            st.ident = None;
        }
        // Delegate to `open` for the argument handling and block form.
        let table = crate::builtins::registered_table(zeo_abi::SYSLOG_MODULE)
            .and_then(|t| t.class.as_ref())
            .and_then(|t| (t.lookup)("open"))
            .expect("Syslog.open is defined");
        table(recv, args, block)
    }

    def self."opened?" (_recv, *args, &_block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(STATE.lock().ident.is_some()))
    }

    def self."close" (_recv, *args, &_block) {
        arity!(args, 0);
        let mut st = STATE.lock();
        if st.ident.is_none() {
            return Err(runtime_error!("syslog not opened"));
        }
        // SAFETY: plain closelog; the kept ident drops with the assignment.
        unsafe { libc::closelog() };
        st.ident = None;
        Ok(RubyValue::Nil)
    }

    // The connection's parameters -- nil while closed, including the ident
    // (CRuby forgets it on close).
    def self."ident" (_recv, *args, &_block) {
        arity!(args, 0);
        Ok(match &STATE.lock().ident {
            Some(c) => RubyValue::Str(crate::string_new(c.to_string_lossy().into_owned())),
            None => RubyValue::Nil,
        })
    }
    def self."options" (_recv, *args, &_block) {
        arity!(args, 0);
        let st = STATE.lock();
        Ok(if st.ident.is_some() { RubyValue::Int(st.options) } else { RubyValue::Nil })
    }
    def self."facility" (_recv, *args, &_block) {
        arity!(args, 0);
        let st = STATE.lock();
        Ok(if st.ident.is_some() { RubyValue::Int(st.facility) } else { RubyValue::Nil })
    }

    // The process log mask. `setlogmask(0)` is the documented read-without-
    // modify spelling.
    def self."mask" (_recv, *args, &_block) {
        arity!(args, 0);
        if STATE.lock().ident.is_none() {
            return Ok(RubyValue::Nil);
        }
        // SAFETY: a 0 argument only reads the current mask.
        Ok(RubyValue::Int(i64::from(unsafe { libc::setlogmask(0) })))
    }
    def self."mask=" (_recv, *args, &_block) {
        arity!(args, 1);
        if STATE.lock().ident.is_none() {
            return Err(runtime_error!("must open syslog before setting log mask"));
        }
        let m = convert::to_index(&args[0])?;
        // SAFETY: plain setlogmask with the caller's mask.
        unsafe { libc::setlogmask(m as libc::c_int) };
        Ok(args[0].clone())
    }

    // `Syslog.log(priority, format, *args)` and the per-priority shortcuts.
    def self."log" arity -1 (_recv, *args, &_block) {
        min_arity(args, 2)?;
        log_with(convert::to_index(&args[0])?, &args[1..])
    }
    def self."emerg" arity -1 (_recv, *args, &_block) {
        min_arity(args, 1)?;
        log_with(libc::LOG_EMERG as i64, args)
    }
    def self."alert" arity -1 (_recv, *args, &_block) {
        min_arity(args, 1)?;
        log_with(libc::LOG_ALERT as i64, args)
    }
    def self."crit" arity -1 (_recv, *args, &_block) {
        min_arity(args, 1)?;
        log_with(libc::LOG_CRIT as i64, args)
    }
    def self."err" arity -1 (_recv, *args, &_block) {
        min_arity(args, 1)?;
        log_with(libc::LOG_ERR as i64, args)
    }
    def self."warning" arity -1 (_recv, *args, &_block) {
        min_arity(args, 1)?;
        log_with(libc::LOG_WARNING as i64, args)
    }
    def self."notice" arity -1 (_recv, *args, &_block) {
        min_arity(args, 1)?;
        log_with(libc::LOG_NOTICE as i64, args)
    }
    def self."info" arity -1 (_recv, *args, &_block) {
        min_arity(args, 1)?;
        log_with(libc::LOG_INFO as i64, args)
    }
    def self."debug" arity -1 (_recv, *args, &_block) {
        min_arity(args, 1)?;
        log_with(libc::LOG_DEBUG as i64, args)
    }

    // `instance` answers the module itself -- the Singleton-flavored spelling
    // some callers use.
    def self."instance" (_recv, *args, &_block) {
        arity!(args, 0);
        Ok(syslog_value())
    }

    // The priority-mask macros, CAPITALIZED METHODS as in CRuby's Macros.
    def self."LOG_MASK" arity 1 (_recv, *args, &_block) {
        arity!(args, 1);
        Ok(RubyValue::Int(1 << convert::to_index(&args[0])?))
    }
    def self."LOG_UPTO" arity 1 (_recv, *args, &_block) {
        arity!(args, 1);
        Ok(RubyValue::Int((1 << (convert::to_index(&args[0])? + 1)) - 1))
    }

    // CRuby's custom shape, not the default module inspect.
    def self."inspect" (_recv, *args, &_block) {
        arity!(args, 0);
        let st = STATE.lock();
        let text = match &st.ident {
            None => "<#Syslog: opened=false>".to_string(),
            Some(c) => {
                // SAFETY: a 0 argument only reads the current mask.
                let mask = unsafe { libc::setlogmask(0) };
                format!(
                    "<#Syslog: opened=true, ident=\"{}\", options={}, facility={}, mask={}>",
                    c.to_string_lossy(),
                    st.options,
                    st.facility,
                    mask
                )
            }
        };
        Ok(RubyValue::Str(crate::string_new(text)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The module's methods have mangled Rust idents; the tests reach them
    /// through the registered lookup, as `Zlib`'s do.
    fn f(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(zeo_abi::SYSLOG_MODULE)
            .expect("Syslog is a registered builtin table")
            .class
            .as_ref()
            .expect("Syslog has module functions");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("Syslog.{name} is defined"))
    }

    fn int(v: Result<RubyValue, Signal>) -> i64 {
        match v.unwrap() {
            RubyValue::Int(i) => i,
            other => panic!("expected Int, got {other:?}"),
        }
    }

    /// Whether the call refused. Raising is registry-backed, and a bare unit
    /// test has no registry, so the raise arrives as a panic rather than an
    /// `Err` -- either one is the refusal being asserted.
    fn refuses(f: impl FnOnce() -> Result<RubyValue, Signal>) -> bool {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
            Err(_) => true,
            Ok(r) => r.is_err(),
        }
    }

    #[test]
    fn the_mask_macros_match_syslog_h() {
        // LOG_MASK(LOG_ERR) == 8, LOG_UPTO(LOG_ERR) == 15 -- ruby 4.0.5.
        assert_eq!(
            int(f("LOG_MASK")(&RubyValue::Nil, &[RubyValue::Int(3)], None)),
            8
        );
        assert_eq!(
            int(f("LOG_UPTO")(&RubyValue::Nil, &[RubyValue::Int(3)], None)),
            15
        );
    }

    /// The whole lifecycle in ONE test, because the connection is process-wide
    /// state and parallel tests would race it: closed-state answers, open
    /// defaults, double-open refusal, a real write, mask, close.
    #[test]
    fn the_connection_lifecycle_matches_cruby() {
        super::install_constants();
        let nil = RubyValue::Nil;

        assert!(matches!(
            f("opened?")(&nil, &[], None).unwrap(),
            RubyValue::Bool(false)
        ));
        assert!(matches!(
            f("ident")(&nil, &[], None).unwrap(),
            RubyValue::Nil
        ));
        assert!(matches!(
            f("mask")(&nil, &[], None).unwrap(),
            RubyValue::Nil
        ));
        assert!(refuses(|| log_with(
            6,
            &[RubyValue::Str(crate::string_new("x".into()))]
        )));

        let ident = RubyValue::Str(crate::string_new("zeo-syslog-test".to_string()));
        f("open")(&nil, std::slice::from_ref(&ident), None).unwrap();
        assert!(matches!(
            f("opened?")(&nil, &[], None).unwrap(),
            RubyValue::Bool(true)
        ));
        assert_eq!(
            int(f("options")(&nil, &[], None)),
            i64::from(libc::LOG_PID | libc::LOG_CONS)
        );
        assert_eq!(
            int(f("facility")(&nil, &[], None)),
            i64::from(libc::LOG_USER)
        );
        assert_eq!(int(f("mask")(&nil, &[], None)), 255);
        assert!(
            refuses(|| f("open")(&nil, &[ident], None)),
            "double open must refuse"
        );

        let fmt = RubyValue::Str(crate::string_new("zeo syslog unit probe %d".to_string()));
        f("log")(&nil, &[RubyValue::Int(6), fmt, RubyValue::Int(42)], None).unwrap();

        f("mask=")(&nil, &[RubyValue::Int(31)], None).unwrap();
        assert_eq!(int(f("mask")(&nil, &[], None)), 31);

        f("close")(&nil, &[], None).unwrap();
        assert!(matches!(
            f("ident")(&nil, &[], None).unwrap(),
            RubyValue::Nil
        ));
        assert!(
            refuses(|| f("close")(&nil, &[], None)),
            "double close must refuse"
        );
    }
}
