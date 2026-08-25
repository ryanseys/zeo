//! Loading a compiled extension: `dlopen`, then `Init_<name>`.
//!
//! The compiler built the shared object (`crates/zeo/src/cext/`) and emitted
//! this call where the `require` sat. What is left is the two steps CRuby's
//! `dln_load` takes, and one that is zeo's own.
//!
//! # Arming the GVL is not optional
//!
//! Ruby code compiled by zeo runs without a global lock whenever it can prove
//! it is alone. A C extension breaks that proof the moment it loads: it may
//! start a thread, and it certainly holds Ruby objects in C locals no other
//! thread's view accounts for. So loading ARMS the GVL, process-wide, before
//! `Init_` runs -- and that is a cost worth naming rather than hiding.
//!
//! # Once per library, by path
//!
//! `Init_` is not idempotent: it calls `rb_define_method` again, which is
//! harmless, and `rb_define_class` again, which is not -- a second call
//! answers a NEW class and the first one's methods vanish from view. CRuby
//! keeps `$LOADED_FEATURES` for the same reason. A second load of the same
//! path is a no-op here.

use crate::Signal;
use std::collections::HashSet;
use std::ffi::{CString, c_char, c_void};
use std::sync::Mutex;

/// Every library already loaded, by the path `dlopen` was given.
static LOADED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

/// `dlopen(path)`, then call `Init_<init>`.
///
/// Answers whether it loaded -- false for a library already in, which is what
/// `require` answers for a feature already loaded.
pub fn load(path: &str, init: &str) -> Result<bool, Signal> {
    let already = LOADED
        .lock()
        .ok()
        .is_some_and(|mut g| !g.get_or_insert_with(HashSet::new).insert(path.to_string()));
    if already {
        return Ok(false);
    }

    let cpath = CString::new(path)
        .map_err(|_| crate::builtins::load_error!("path contains a null byte: {path}"))?;
    let handle = open_library(&cpath)
        .map_err(|why| crate::builtins::load_error!("cannot load such file -- {path}: {why}"))?;

    let symbol = format!("Init_{init}");
    let csym = CString::new(symbol.clone())
        .map_err(|_| crate::builtins::load_error!("bad init name: {symbol}"))?;
    // SAFETY: a live handle and a NUL-terminated name; a miss answers null.
    let entry = unsafe { libc::dlsym(handle, csym.as_ptr()) };
    if entry.is_null() {
        return Err(crate::builtins::load_error!(
            "{path} has no {symbol} -- is this a Ruby extension?"
        ));
    }

    // Before `Init_` runs, and in this order: the globals an `Init_` reads on
    // its first line (`rb_define_class_under(rb_cObject, ...)`), then the
    // GVL, then the scope its handles are pinned in.
    super::globals::fill();
    // A `false` means the process Gvl was already created disabled, which
    // only a program that spawned a thread before its first `require` can
    // reach. The sole-thread fast path is off either way, and that is the
    // half an extension can corrupt.
    let _armed = crate::gvl::arm_for_cext();
    let scope = super::scope::Scope::enter();
    // SAFETY: `dlsym` answered, and `Init_` is `void (*)(void)` by MRI's own
    // contract -- the name is what the extension's build asserts.
    let init_fn: unsafe extern "C" fn() = unsafe { std::mem::transmute(entry) };
    let out = super::jmp::protect(|| unsafe { init_fn() });
    drop(scope);
    out?;
    Ok(true)
}

/// `dlopen`, or the dynamic loader's own reason.
///
/// `RTLD_NOW | RTLD_GLOBAL`.
///
/// `RTLD_NOW` rather than MRI's `RTLD_LAZY`, and the difference is the whole
/// value of `cext/stubs.rs`. Lazily, a `rb_*` zeo does not export binds to
/// nothing and the extension SIGSEGVs at the call -- no symbol name, no
/// backtrace, no way to tell a zeo gap from a bug in the gem. `fast_blank`
/// did exactly that when the census had not seen `ruby/encoding.h`.
/// Resolving eagerly turns the same gap into a `LoadError` naming the
/// symbol, which is the promise the stub file exists to keep.
///
/// `RTLD_GLOBAL` because a second extension may reference the first's
/// symbols -- which is what `rb_ext_resolve_symbol` is for.
///
/// A `Result<_, String>` rather than a `Signal`: building the `LoadError`
/// needs the class registry, and this is the half a unit test can look at.
fn open_library(path: &std::ffi::CStr) -> Result<*mut c_void, String> {
    // SAFETY: a NUL-terminated path; a failure answers null and is reported.
    let handle = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL) };
    if handle.is_null() {
        return Err(dlerror());
    }
    Ok(handle)
}

fn dlerror() -> String {
    // SAFETY: `dlerror` answers a NUL-terminated string or null, and the
    // string is valid until the next call on this thread.
    let p = unsafe { libc::dlerror() } as *const c_char;
    if p.is_null() {
        return "unknown error".into();
    }
    // SAFETY: as above.
    unsafe { std::ffi::CStr::from_ptr(p) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A second load of one path is a no-op. `Init_` calling
    /// `rb_define_class` twice answers a NEW class the second time, and the
    /// first one's methods vanish from view -- so this is not an
    /// optimisation.
    #[test]
    fn a_path_loads_at_most_once() {
        let path = "/zeo/test/only-the-table-is-on-trial.bundle";
        if let Ok(mut g) = LOADED.lock() {
            g.get_or_insert_with(HashSet::new).insert(path.to_string());
        }
        // The second call short-circuits before `dlopen`, so a path that
        // does not exist still answers rather than raising.
        assert_eq!(load(path, "whatever").ok(), Some(false));
    }

    /// A missing library is refused with the dynamic loader's own reason,
    /// which is what makes a `LoadError` around an optional native half
    /// actionable. `load` wraps this in the exception; building one needs
    /// the class registry, so the decision is what is on trial.
    #[test]
    fn a_missing_library_is_refused_with_the_loaders_reason() {
        let path = CString::new("/zeo/test/definitely-absent.bundle").expect("no NUL");
        let why = open_library(&path).expect_err("a missing library opened");
        assert!(
            why.contains("definitely-absent"),
            "the reason does not name the path: {why}"
        );
    }

    /// The process's own image always opens, so a null path is the one
    /// `dlopen` call that must NOT be mistaken for a failure -- and zeo
    /// never makes it, because a null path means "this executable".
    ///
    /// The subject differs per OS: glibc refuses to `dlopen` a PIE
    /// executable by path (every Rust test binary is one), so Linux opens
    /// the C library the process already maps instead.
    #[test]
    fn the_loader_reports_success_as_a_handle() {
        #[cfg(target_os = "linux")]
        let me = CString::new("libc.so.6").expect("no NUL");
        #[cfg(not(target_os = "linux"))]
        let me = CString::new(
            std::env::current_exe().map_or(String::new(), |p| p.to_string_lossy().into_owned()),
        )
        .expect("an executable path has no NUL");
        assert!(open_library(&me).is_ok(), "the running image did not open");
    }
}
