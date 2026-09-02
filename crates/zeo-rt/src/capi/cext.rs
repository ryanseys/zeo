//! The emitted call that loads a gem's compiled C extension.
//!
//! This lives here rather than in the C-API crate because the capi symbol
//! table is not optional: every emitted program resolves its imports
//! against one fixed list, and a row that appeared and vanished with the
//! build would make "the emitter can call it" depend on how the runtime
//! was configured.
//!
//! So the symbol always exists. What varies is the answer: a binary built
//! without the C-API crate says so, naming the library it was asked for,
//! rather than failing to link (`capi_hooks::load`).

use zeo_abi::abi::{STATUS_OK, STATUS_SIGNAL};

/// `dlopen` the extension at `path` and run its `Init_<init>`.
///
/// # Safety
///
/// Both pointers must name their stated number of readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zeo_rt_cext_load(
    path: *const u8,
    path_len: usize,
    init: *const u8,
    init_len: usize,
) -> i32 {
    let path = unsafe { super::str_slice(path, path_len) };
    let init = unsafe { super::str_slice(init, init_len) };
    match load(path, init) {
        Ok(()) => STATUS_OK,
        Err(sig) => {
            crate::signal::set_pending(sig);
            STATUS_SIGNAL
        }
    }
}

fn load(path: &str, init: &str) -> Result<(), crate::Signal> {
    crate::capi_hooks::load(path, init).map(|_| ())
}
