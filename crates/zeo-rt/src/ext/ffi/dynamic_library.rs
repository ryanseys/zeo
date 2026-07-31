//! `FFI::DynamicLibrary` -- `dlopen(3)` handles. `.open(name, flags)` opens a
//! shared library (or, with `nil`, the whole process image -- how fiddle's
//! `Fiddle.dlopen(nil)` reaches every libc symbol), and `#find_function`
//! resolves a symbol through `dlsym(3)` to an `FFI::Pointer`. A failed open
//! raises `LoadError` carrying the raw `dlerror(3)` text, which is exactly the
//! message CRuby's fiddle surfaces in its `DLError`.
//!
//! Handles are never `dlclose`d: they live for the process, matching how the
//! gem (and CRuby's own extension loader) treats them in practice.

use std::ffi::CString;
use std::os::raw::c_void;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::wrap_address;
use crate::builtins::{arg_error, convert::to_rstr};
use crate::dispatch::{RObj, RubyObject, raise_error};
use crate::{ClassId, RubyValue};
use zeo_abi::FFI_DYNAMIC_LIBRARY_CLASS;
use zeo_macros::ruby_class;

pub struct RDynLib {
    handle: *mut c_void,
    name: Option<String>,
    frozen: AtomicBool,
}

// A dlopen handle is process-global state; dlsym on it is thread-safe.
unsafe impl Send for RDynLib {}
unsafe impl Sync for RDynLib {}

impl RubyObject for RDynLib {
    fn class_id(&self) -> ClassId {
        FFI_DYNAMIC_LIBRARY_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        self.frozen.load(std::sync::atomic::Ordering::Relaxed)
    }
    fn set_frozen(&self) {
        self.frozen
            .store(true, std::sync::atomic::Ordering::Relaxed)
    }
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, copy_frozen: bool) -> RObj {
        let d = RDynLib {
            handle: self.handle,
            name: self.name.clone(),
            frozen: AtomicBool::new(false),
        };
        if copy_frozen {
            d.set_frozen();
        }
        Arc::new(d)
    }
}

fn lib_of(recv: &RubyValue) -> &RDynLib {
    match recv {
        RubyValue::Object(o) => o
            .as_any()
            .downcast_ref::<RDynLib>()
            .expect("the DynamicLibrary table only dispatches on library receivers"),
        _ => unreachable!("the DynamicLibrary table only dispatches on library receivers"),
    }
}

/// The current `dlerror(3)` text (cleared by the read, per its contract).
fn dlerror_string() -> String {
    let p = unsafe { libc::dlerror() };
    if p.is_null() {
        return "dlopen failed".to_string();
    }
    unsafe { std::ffi::CStr::from_ptr(p) }
        .to_string_lossy()
        .into_owned()
}

ruby_class! {
    DynamicLibrary = zeo_abi::FFI_DYNAMIC_LIBRARY_CLASS < zeo_abi::OBJECT_CLASS;

    const RTLD_LAZY = RubyValue::Int(libc::RTLD_LAZY as i64);
    const RTLD_NOW = RubyValue::Int(libc::RTLD_NOW as i64);
    const RTLD_GLOBAL = RubyValue::Int(libc::RTLD_GLOBAL as i64);
    const RTLD_LOCAL = RubyValue::Int(libc::RTLD_LOCAL as i64);

    // `open(libname, flags)` -- `nil` opens the process image. A failure is a
    // `LoadError` whose message is the raw `dlerror` text.
    def self."open"(_recv, arg1, arg2) {
        let name = match arg1 {
            RubyValue::Nil => None,
            v => Some(to_rstr(v)?.lock().to_string()),
        };
        let flags = match arg2 {
            RubyValue::Int(i) => *i as libc::c_int,
            RubyValue::Nil => libc::RTLD_LAZY,
            _ => return Err(arg_error!("dlopen flags must be an Integer")),
        };
        let cname = match &name {
            Some(n) => Some(
                CString::new(n.as_bytes()).map_err(|_| arg_error!("string contains null byte"))?,
            ),
            None => None,
        };
        let handle = unsafe {
            libc::dlopen(
                cname.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()),
                flags,
            )
        };
        if handle.is_null() {
            return Err(raise_error("LoadError", dlerror_string()));
        }
        Ok(RubyValue::Object(Arc::new(RDynLib {
            handle,
            name,
            frozen: AtomicBool::new(false),
        })))
    }

    // `dlsym` the name; a missing symbol answers a NULL `Pointer` (the gem's
    // callers test `#null?`).
    def "find_function" | "find_variable"(recv, arg) {
        let name = to_rstr(arg)?.lock().to_string();
        let cname =
            CString::new(name).map_err(|_| arg_error!("string contains null byte"))?;
        let addr = unsafe { libc::dlsym(lib_of(recv).handle, cname.as_ptr()) };
        Ok(wrap_address(addr as usize))
    }

    def "name"(recv) {
        Ok(match &lib_of(recv).name {
            Some(n) => RubyValue::Str(crate::string_new(n.clone())),
            None => RubyValue::Nil,
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn process_image_resolves_libc_symbols() {
        let h = unsafe { libc::dlopen(std::ptr::null(), libc::RTLD_LAZY) };
        assert!(!h.is_null());
        let sym = unsafe { libc::dlsym(h, c"strlen".as_ptr()) };
        assert!(!sym.is_null());
        let missing = unsafe { libc::dlsym(h, c"zzz_no_such_symbol_zeo".as_ptr()) };
        assert!(missing.is_null());
    }
}
