//! The process-wide FFI vocabulary a snippet's compile seeds from.
//!
//! An FFI field type has to resolve to a byte width and an OFFSET at lowering
//! time, and the tables that resolve it (`Hir::ffi_types`,
//! `Hir::ffi_struct_layouts`) belong to ONE compile. A snippet is its own
//! compile, so `eval "class Outer < FFI::Struct; layout :pt, Pt; end"` had
//! never heard of a `Pt` an earlier snippet -- or the program itself --
//! declared, even though the CLASS is right there at run time.
//!
//! Every compile in the process publishes what it declared here, and a
//! snippet's compile seeds from it. That is sound because the vocabulary is
//! pure compile-time DESCRIPTION -- names to widths, offsets and field lists
//! -- with no reference to any one arena: a layout names its fields by string
//! and its types by `FfiType`, both of which clone freely.
//!
//! Seeding a WHOLE-PROGRAM compile would be wrong, and it does not happen: a
//! program is compiled once, before anything has run, and letting one
//! program's declarations reach another's would make a compile depend on what
//! else the process had done.

use std::sync::Mutex;

use crate::compiler::FMap;
use crate::hir::{FfiStructLayout, FfiType, Hir};

#[derive(Default)]
struct Vocab {
    /// `ffi_types`' shape verbatim, poison arm included: `None` is a leaf
    /// rebound to a DIFFERENT type, which resolves to nothing.
    types: FMap<String, Option<FfiType>>,
    layouts: FMap<String, FfiStructLayout>,
}

static VOCAB: Mutex<Option<Vocab>> = Mutex::new(None);

/// Seed `hir` with everything earlier compiles in this process declared.
/// Existing entries win: the snippet's own declarations are lowered after
/// this and overwrite, which is the order a second `class Pt` should take.
pub(crate) fn seed(hir: &mut Hir) {
    let guard = VOCAB.lock().expect("the ffi vocabulary is never poisoned");
    let Some(v) = guard.as_ref() else {
        return;
    };
    for (k, t) in &v.types {
        hir.ffi_types.entry(k.clone()).or_insert_with(|| t.clone());
    }
    for (k, l) in &v.layouts {
        hir.ffi_struct_layouts
            .entry(k.clone())
            .or_insert_with(|| l.clone());
    }
}

/// Record what `hir` declared, for the snippets that follow it.
pub(crate) fn publish(hir: &Hir) {
    if hir.ffi_types.is_empty() && hir.ffi_struct_layouts.is_empty() {
        return;
    }
    let mut guard = VOCAB.lock().expect("the ffi vocabulary is never poisoned");
    let v = guard.get_or_insert_with(Vocab::default);
    for (k, t) in &hir.ffi_types {
        v.types.insert(k.clone(), t.clone());
    }
    for (k, l) in &hir.ffi_struct_layouts {
        v.layouts.insert(k.clone(), l.clone());
    }
}
