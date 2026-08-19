//! The single table of runtime symbols emitted code imports, with their
//! C signatures -- what `emit` declares imports from, and what the
//! `capi_surface` test (M0-16) asserts against `zeo-rt`'s exports. A
//! symbol used by any lowering MUST come from here; a name the runtime
//! does not export fails the link, and this table is where that name is
//! grepped for.

/// A C parameter/return slot, mapped to a Cranelift type by `emit`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CTy {
    /// Any pointer (`*const`/`*mut`) -- the target's pointer type.
    Ptr,
    /// `usize` -- pointer-width integer.
    Usize,
    /// `i32` (the status protocol, C `int`).
    I32,
    /// `u32` (class/symbol ids, line numbers).
    U32,
    /// `u8` (encoding ids, flag bytes) -- unsigned-extended per the C ABI.
    U8,
}

/// One imported runtime function: its exact exported name and C shape.
pub struct CapiSig {
    pub name: &'static str,
    pub params: &'static [CTy],
    pub ret: Option<CTy>,
}

use CTy::{I32, Ptr, U8, U32, Usize};

/// Every runtime symbol the emitter can import, alphabetical by name.
/// Grows with the lowerings; `sig` panics on a name not listed -- an
/// internal compiler error, never a user-visible path.
pub const CAPI: &[CapiSig] = &[
    CapiSig {
        name: "zeo_rt_check_ints",
        params: &[],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_frame_pop",
        params: &[],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_frame_push",
        params: &[Ptr, Usize, Ptr, Usize, U32, U32],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_kernel_puts",
        params: &[Ptr, Usize, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_main",
        params: &[I32, Ptr, Ptr, Ptr],
        ret: Some(I32),
    },
    CapiSig {
        name: "zeo_rt_pool_push",
        params: &[Ptr],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_set_line",
        params: &[U32],
        ret: None,
    },
    CapiSig {
        name: "zeo_rt_str_new",
        params: &[Ptr, Usize, U8, Ptr],
        ret: None,
    },
];

/// The signature row for `name`.
pub fn sig(name: &str) -> &'static CapiSig {
    CAPI.iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("capi_names: no signature row for {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_sorted_and_unique() {
        for pair in CAPI.windows(2) {
            assert!(
                pair[0].name < pair[1].name,
                "{} must sort before {}",
                pair[0].name,
                pair[1].name
            );
        }
    }
}
