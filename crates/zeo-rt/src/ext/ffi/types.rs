//! `FFI::Type` / `FFI::Type::Builtin` -- the gem's runtime type objects.
//! Each canonical scalar type is a single shared instance exposed as a
//! constant on BOTH classes (the gem aliases them the same way), so identity
//! comparisons like `ffi_args.last == FFI::Type::Builtin::VARARGS` hold;
//! `==` also compares by kind, so equal types from different spellings
//! (`LONG` vs `LONG_LONG` on LP64) compare equal exactly as their C types do.
//! fiddle's FFI backend derives its whole `SIZEOF_*`/`ALIGN_*` constant set
//! from `#size`/`#alignment` here.

use std::sync::{Arc, OnceLock};

use crate::builtins::type_error;
use crate::dispatch::{RObj, RubyObject};
use crate::ffi::FfiKind;
use crate::{ClassId, RubyValue, Signal};
use zeo_abi::FFI_TYPE_BUILTIN_CLASS;
use zeo_macros::ruby_class;

/// An `FFI::Type` instance's payload: the scalar kind it names, or the
/// `VARARGS` marker (which has no C size -- it terminates a type list).
pub struct RType {
    pub kind: TypeKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TypeKind {
    Scalar(FfiKind),
    Varargs,
}

impl RType {
    /// (size, alignment) of the C type, LP64. `VOID` reports 1/1 as libffi's
    /// `ffi_type_void` does; `VARARGS` is never asked (0/0 if it were).
    fn layout(&self) -> (usize, usize) {
        use FfiKind::*;
        match self.kind {
            TypeKind::Varargs => (0, 0),
            TypeKind::Scalar(k) => match k {
                Void => (1, 1),
                I8 | U8 | Bool => (1, 1),
                I16 | U16 => (2, 2),
                I32 | U32 => (4, 4),
                F32 => (4, 4),
                I64 | U64 | F64 | Str | Pointer => (8, 8),
            },
        }
    }
}

impl RubyObject for RType {
    fn class_id(&self) -> ClassId {
        FFI_TYPE_BUILTIN_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    // The canonical instances are shared constants -- permanently frozen.
    fn is_frozen(&self) -> bool {
        true
    }
    fn set_frozen(&self) {}
    fn ivar_values(&self) -> Vec<RubyValue> {
        Vec::new()
    }
    fn dup_object(&self, _copy_frozen: bool) -> RObj {
        Arc::new(RType { kind: self.kind })
    }
}

/// The canonical instance for `kind` (one shared `Arc` per kind, so the
/// constants on `Type` and `Type::Builtin` are the SAME object).
pub fn type_value(kind: TypeKind) -> RubyValue {
    static CANON: OnceLock<Vec<(TypeKind, RubyValue)>> = OnceLock::new();
    let table = CANON.get_or_init(|| {
        use FfiKind::*;
        [
            Void, I8, U8, I16, U16, I32, U32, I64, U64, F32, F64, Bool, Str, Pointer,
        ]
        .into_iter()
        .map(TypeKind::Scalar)
        .chain([TypeKind::Varargs])
        .map(|k| (k, RubyValue::Object(Arc::new(RType { kind: k }))))
        .collect()
    });
    table
        .iter()
        .find(|(k, _)| *k == kind)
        .expect("every TypeKind is seeded")
        .1
        .clone()
}

/// The `FfiKind` a Ruby value names as an FFI type: an `FFI::Type` object, or
/// a Symbol spelling (`:int`). `Err` for `VARARGS` and non-types -- callers
/// that accept the varargs marker test with [`is_varargs_type`] first.
pub fn kind_of_type_value(v: &RubyValue) -> Result<FfiKind, Signal> {
    if let Some(t) = rtype_of(v) {
        return match t.kind {
            TypeKind::Scalar(k) => Ok(k),
            TypeKind::Varargs => Err(type_error!("`:varargs` is not a concrete FFI type")),
        };
    }
    if let RubyValue::Symbol(s) = v {
        return crate::ffi::kind_from_symbol(&s.name());
    }
    Err(type_error!(
        "wrong argument type {} (expected an FFI type)",
        crate::builtins::class_name_of(v)
    ))
}

/// Whether `v` is the `FFI::Type::Builtin::VARARGS` marker.
pub fn is_varargs_type(v: &RubyValue) -> bool {
    matches!(
        rtype_of(v),
        Some(RType {
            kind: TypeKind::Varargs
        })
    )
}

fn rtype_of(v: &RubyValue) -> Option<&RType> {
    match v {
        RubyValue::Object(o) => o.as_any().downcast_ref::<RType>(),
        _ => None,
    }
}

fn t_of(recv: &RubyValue) -> &RType {
    rtype_of(recv).expect("the FFI::Type table only dispatches on type receivers")
}

// Both classes expose the same canonical constants (the gem defines both
// spellings); each block writes the list out.
ruby_class! {
    Type = zeo_abi::FFI_TYPE_CLASS < zeo_abi::OBJECT_CLASS;

    const VOID = type_value(TypeKind::Scalar(FfiKind::Void));
    const POINTER = type_value(TypeKind::Scalar(FfiKind::Pointer));
    const STRING = type_value(TypeKind::Scalar(FfiKind::Str));
    const BOOL = type_value(TypeKind::Scalar(FfiKind::Bool));
    const VARARGS = type_value(TypeKind::Varargs);
    const CHAR = type_value(TypeKind::Scalar(FfiKind::I8));
    const UCHAR = type_value(TypeKind::Scalar(FfiKind::U8));
    const SHORT = type_value(TypeKind::Scalar(FfiKind::I16));
    const USHORT = type_value(TypeKind::Scalar(FfiKind::U16));
    const INT = type_value(TypeKind::Scalar(FfiKind::I32));
    const UINT = type_value(TypeKind::Scalar(FfiKind::U32));
    const LONG = type_value(TypeKind::Scalar(FfiKind::I64));
    const ULONG = type_value(TypeKind::Scalar(FfiKind::U64));
    const LONG_LONG = type_value(TypeKind::Scalar(FfiKind::I64));
    const ULONG_LONG = type_value(TypeKind::Scalar(FfiKind::U64));
    const INT8 = type_value(TypeKind::Scalar(FfiKind::I8));
    const UINT8 = type_value(TypeKind::Scalar(FfiKind::U8));
    const INT16 = type_value(TypeKind::Scalar(FfiKind::I16));
    const UINT16 = type_value(TypeKind::Scalar(FfiKind::U16));
    const INT32 = type_value(TypeKind::Scalar(FfiKind::I32));
    const UINT32 = type_value(TypeKind::Scalar(FfiKind::U32));
    const INT64 = type_value(TypeKind::Scalar(FfiKind::I64));
    const UINT64 = type_value(TypeKind::Scalar(FfiKind::U64));
    const FLOAT = type_value(TypeKind::Scalar(FfiKind::F32));
    const FLOAT32 = type_value(TypeKind::Scalar(FfiKind::F32));
    const DOUBLE = type_value(TypeKind::Scalar(FfiKind::F64));
    const FLOAT64 = type_value(TypeKind::Scalar(FfiKind::F64));

    def "size"(recv) {
        Ok(RubyValue::Int(t_of(recv).layout().0 as i64))
    }
    def "alignment"(recv) {
        Ok(RubyValue::Int(t_of(recv).layout().1 as i64))
    }
    def "==" | "eql?"(recv, other) {
        Ok(RubyValue::Bool(matches!(rtype_of(other), Some(o) if o.kind == t_of(recv).kind)))
    }
}

// The DSL emits one `install_constants` per block, so the second class lives
// in its own module.
mod builtin {
    use super::*;

    ruby_class! {
    TypeBuiltin = zeo_abi::FFI_TYPE_BUILTIN_CLASS < zeo_abi::FFI_TYPE_CLASS;

    const VOID = type_value(TypeKind::Scalar(FfiKind::Void));
    const POINTER = type_value(TypeKind::Scalar(FfiKind::Pointer));
    const STRING = type_value(TypeKind::Scalar(FfiKind::Str));
    const BOOL = type_value(TypeKind::Scalar(FfiKind::Bool));
    const VARARGS = type_value(TypeKind::Varargs);
    const CHAR = type_value(TypeKind::Scalar(FfiKind::I8));
    const UCHAR = type_value(TypeKind::Scalar(FfiKind::U8));
    const SHORT = type_value(TypeKind::Scalar(FfiKind::I16));
    const USHORT = type_value(TypeKind::Scalar(FfiKind::U16));
    const INT = type_value(TypeKind::Scalar(FfiKind::I32));
    const UINT = type_value(TypeKind::Scalar(FfiKind::U32));
    const LONG = type_value(TypeKind::Scalar(FfiKind::I64));
    const ULONG = type_value(TypeKind::Scalar(FfiKind::U64));
    const LONG_LONG = type_value(TypeKind::Scalar(FfiKind::I64));
    const ULONG_LONG = type_value(TypeKind::Scalar(FfiKind::U64));
    const INT8 = type_value(TypeKind::Scalar(FfiKind::I8));
    const UINT8 = type_value(TypeKind::Scalar(FfiKind::U8));
    const INT16 = type_value(TypeKind::Scalar(FfiKind::I16));
    const UINT16 = type_value(TypeKind::Scalar(FfiKind::U16));
    const INT32 = type_value(TypeKind::Scalar(FfiKind::I32));
    const UINT32 = type_value(TypeKind::Scalar(FfiKind::U32));
    const INT64 = type_value(TypeKind::Scalar(FfiKind::I64));
    const UINT64 = type_value(TypeKind::Scalar(FfiKind::U64));
    const FLOAT = type_value(TypeKind::Scalar(FfiKind::F32));
    const FLOAT32 = type_value(TypeKind::Scalar(FfiKind::F32));
    const DOUBLE = type_value(TypeKind::Scalar(FfiKind::F64));
    const FLOAT64 = type_value(TypeKind::Scalar(FfiKind::F64));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_instances_are_shared() {
        let a = type_value(TypeKind::Scalar(FfiKind::I32));
        let b = type_value(TypeKind::Scalar(FfiKind::I32));
        let (RubyValue::Object(x), RubyValue::Object(y)) = (&a, &b) else {
            panic!("type_value answers objects");
        };
        assert_eq!(
            Arc::as_ptr(x) as *const u8 as usize,
            Arc::as_ptr(y) as *const u8 as usize
        );
    }

    #[test]
    fn layouts_are_lp64() {
        let t = |k| RType {
            kind: TypeKind::Scalar(k),
        };
        assert_eq!(t(FfiKind::I32).layout(), (4, 4));
        assert_eq!(t(FfiKind::I64).layout(), (8, 8));
        assert_eq!(t(FfiKind::Bool).layout(), (1, 1));
        assert_eq!(t(FfiKind::F32).layout(), (4, 4));
        assert_eq!(t(FfiKind::Pointer).layout(), (8, 8));
    }

    #[test]
    fn kind_resolution_accepts_types_and_symbols() {
        let v = type_value(TypeKind::Scalar(FfiKind::F64));
        assert!(matches!(kind_of_type_value(&v), Ok(FfiKind::F64)));
        let s = RubyValue::Symbol(crate::Symbol::intern("int"));
        assert!(matches!(kind_of_type_value(&s), Ok(FfiKind::I32)));
        // (a Varargs argument errors, but constructing the TypeError needs
        // the exception REGISTRY, absent in a unit test -- e2e covers it)
        assert!(is_varargs_type(&type_value(TypeKind::Varargs)));
    }
}
