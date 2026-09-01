//! `FFI::Type` / `FFI::Type::Builtin` -- the gem's runtime type objects.
//!
//! There is exactly one instance per CANONICAL type, and the aliases are
//! extra constants naming the same object: `:char` and `:int8` really are
//! `Type::Builtin::INT8`, which is why `CHAR.inspect` says `INT8`. The
//! canonical set is NOT the set of distinct C widths -- `LONG` and `INT64`
//! are separate objects that compare UNEQUAL although both are eight signed
//! bytes on every target zeo builds for, and so are `ULONG` and `UINT64`.
//! Collapsing them by width made `ffi_args.last == FFI::Type::Builtin::LONG`
//! answer true for an `:int64` argument, which CRuby answers false.
//!
//! fiddle's FFI backend derives its whole `SIZEOF_*`/`ALIGN_*` constant set
//! from `#size`/`#alignment` here.

use std::sync::{Arc, OnceLock};

use crate::builtins::type_error;
use crate::dispatch::{RObj, RubyObject};
use crate::ffi::FfiKind;
use crate::{ClassId, RubyValue, Signal};
use zeo_abi::FFI_TYPE_BUILTIN_CLASS;
use zeo_macros::ruby_class;

/// The canonical builtin types, one per name the gem's `#inspect` can print.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    Void,
    Int8,
    Uint8,
    Int16,
    Uint16,
    Int32,
    Uint32,
    Int64,
    Uint64,
    Long,
    Ulong,
    Float32,
    Float64,
    LongDouble,
    Pointer,
    Bool,
    String,
    BufferIn,
    BufferOut,
    BufferInout,
    Varargs,
}

use Builtin::*;

/// Every canonical type, in the order the constant tables list them.
const ALL: [Builtin; 21] = [
    Void,
    Int8,
    Uint8,
    Int16,
    Uint16,
    Int32,
    Uint32,
    Int64,
    Uint64,
    Long,
    Ulong,
    Float32,
    Float64,
    LongDouble,
    Pointer,
    Bool,
    String,
    BufferIn,
    BufferOut,
    BufferInout,
    Varargs,
];

impl Builtin {
    /// The name `#inspect` prints, which is also the constant naming this
    /// object on `Type::Builtin` and (prefixed with `TYPE_`) on `FFI`.
    fn name(self) -> &'static str {
        match self {
            Void => "VOID",
            Int8 => "INT8",
            Uint8 => "UINT8",
            Int16 => "INT16",
            Uint16 => "UINT16",
            Int32 => "INT32",
            Uint32 => "UINT32",
            Int64 => "INT64",
            Uint64 => "UINT64",
            Long => "LONG",
            Ulong => "ULONG",
            Float32 => "FLOAT32",
            Float64 => "FLOAT64",
            LongDouble => "LONGDOUBLE",
            Pointer => "POINTER",
            Bool => "BOOL",
            String => "STRING",
            BufferIn => "BUFFER_IN",
            BufferOut => "BUFFER_OUT",
            BufferInout => "BUFFER_INOUT",
            Varargs => "VARARGS",
        }
    }

    /// The scalar this type marshals as, or `None` for the two the runtime
    /// tier cannot pass: `VARARGS` terminates a type list rather than
    /// naming a value, and zeo has no `long double`.
    fn kind(self) -> Option<FfiKind> {
        Some(match self {
            Void => FfiKind::Void,
            Int8 => FfiKind::I8,
            Uint8 => FfiKind::U8,
            Int16 => FfiKind::I16,
            Uint16 => FfiKind::U16,
            Int32 => FfiKind::I32,
            Uint32 => FfiKind::U32,
            Int64 | Long => FfiKind::I64,
            Uint64 | Ulong => FfiKind::U64,
            Float32 => FfiKind::F32,
            Float64 => FfiKind::F64,
            Bool => FfiKind::Bool,
            String => FfiKind::Str,
            Pointer | BufferIn | BufferOut | BufferInout => FfiKind::Pointer,
            LongDouble | Varargs => return None,
        })
    }

    /// (size, alignment) in bytes. `VOID` and `VARARGS` report 1/1, as
    /// libffi's `ffi_type_void` does and as the gem prints.
    fn layout(self) -> (usize, usize) {
        match self {
            Void | Varargs => (1, 1),
            // Apple aliases `long double` to `double`; every other target
            // zeo builds for gives it a 16-byte slot.
            // Apple aliases `long double` to `double` on arm64 only; an
            // Intel Mac gives it a 16-byte slot like every other target.
            LongDouble if cfg!(all(target_vendor = "apple", target_arch = "aarch64")) => (8, 8),
            LongDouble => (16, 16),
            other => {
                let k = other.kind().expect("every other type marshals");
                match k {
                    FfiKind::Void => (1, 1),
                    k => (k.size(), k.align()),
                }
            }
        }
    }
}

/// An `FFI::Type` instance's payload: which canonical type it is.
pub struct RType {
    pub builtin: Builtin,
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
        Arc::new(RType {
            builtin: self.builtin,
        })
    }
}

/// The canonical instance for `b` -- one shared `Arc` per canonical type, so
/// every constant naming it (`CHAR`, `SCHAR` and `INT8` all name `INT8`) is
/// the SAME object.
pub fn type_value(b: Builtin) -> RubyValue {
    static CANON: OnceLock<Vec<RubyValue>> = OnceLock::new();
    let table = CANON.get_or_init(|| {
        ALL.into_iter()
            .map(|b| RubyValue::Object(Arc::new(RType { builtin: b })))
            .collect()
    });
    let i = ALL
        .iter()
        .position(|&x| x == b)
        .expect("every Builtin is seeded");
    table[i].clone()
}

/// The `FfiKind` a Ruby value names as an FFI type: an `FFI::Type` object, or
/// a Symbol spelling (`:int`). `Err` for `VARARGS` and non-types -- callers
/// that accept the varargs marker test with [`is_varargs_type`] first.
pub fn kind_of_type_value(v: &RubyValue) -> Result<FfiKind, Signal> {
    if let Some(t) = rtype_of(v) {
        return match t.builtin.kind() {
            Some(k) => Ok(k),
            None if t.builtin == Varargs => {
                Err(type_error!("`:varargs` is not a concrete FFI type"))
            }
            None => Err(type_error!(
                "`:long_double` isn't supported yet (zeo limitation)"
            )),
        };
    }
    if let RubyValue::Symbol(s) = v {
        return crate::ffi::kind_from_symbol(&s.name());
    }
    Err(type_error!(
        "wrong argument type {} (expected an FFI type)",
        crate::builtins::check_type_name(v)
    ))
}

/// [`kind_of_type_value`] for a RETURN position, where `:void` is a type:
/// the gem's `FFI::Function.new(:void, ...)` and `callback :n, [...], :void`
/// both spell a callback that answers nothing.
pub fn return_kind_of_type_value(v: &RubyValue) -> Result<FfiKind, Signal> {
    match v {
        RubyValue::Symbol(s) if s.name() == "void" => Ok(FfiKind::Void),
        _ => kind_of_type_value(v),
    }
}

/// Whether `v` is the `FFI::Type::Builtin::VARARGS` marker.
pub fn is_varargs_type(v: &RubyValue) -> bool {
    matches!(rtype_of(v), Some(t) if t.builtin == Varargs)
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

/// What both classes carry, written once. `Type` and `Type::Builtin` hold the
/// same constants (the gem defines both spellings) and the DSL emits one
/// `install_constants` per block, so each class expands this list in its own
/// block. `inspect` is here rather than inherited because `p` and string
/// interpolation ask the receiver's OWN class -- see `call_user_method`.
macro_rules! builtin_type_class {
    ($name:ident = $class:path, $super:path; $($rest:tt)*) => {
        ruby_class! {
        $name = $class < $super;

        const VOID = type_value(Builtin::Void);
        const POINTER = type_value(Builtin::Pointer);
        const STRING = type_value(Builtin::String);
        const BOOL = type_value(Builtin::Bool);
        const VARARGS = type_value(Builtin::Varargs);
        const BUFFER_IN = type_value(Builtin::BufferIn);
        const BUFFER_OUT = type_value(Builtin::BufferOut);
        const BUFFER_INOUT = type_value(Builtin::BufferInout);
        const LONGDOUBLE = type_value(Builtin::LongDouble);
        // `char`/`short`/`int` name the exact-width types; `long` does not.
        const CHAR = type_value(Builtin::Int8);
        const SCHAR = type_value(Builtin::Int8);
        const UCHAR = type_value(Builtin::Uint8);
        const SHORT = type_value(Builtin::Int16);
        const SSHORT = type_value(Builtin::Int16);
        const USHORT = type_value(Builtin::Uint16);
        const INT = type_value(Builtin::Int32);
        const SINT = type_value(Builtin::Int32);
        const UINT = type_value(Builtin::Uint32);
        const LONG = type_value(Builtin::Long);
        const SLONG = type_value(Builtin::Long);
        const ULONG = type_value(Builtin::Ulong);
        const LONG_LONG = type_value(Builtin::Int64);
        const SLONG_LONG = type_value(Builtin::Int64);
        const ULONG_LONG = type_value(Builtin::Uint64);
        const INT8 = type_value(Builtin::Int8);
        const UINT8 = type_value(Builtin::Uint8);
        const INT16 = type_value(Builtin::Int16);
        const UINT16 = type_value(Builtin::Uint16);
        const INT32 = type_value(Builtin::Int32);
        const UINT32 = type_value(Builtin::Uint32);
        const INT64 = type_value(Builtin::Int64);
        const UINT64 = type_value(Builtin::Uint64);
        const FLOAT = type_value(Builtin::Float32);
        const FLOAT32 = type_value(Builtin::Float32);
        const DOUBLE = type_value(Builtin::Float64);
        const FLOAT64 = type_value(Builtin::Float64);

        def "inspect"(recv) {
            let b = t_of(recv).builtin;
            let (size, align) = b.layout();
            Ok(RubyValue::Str(crate::string_new(format!(
                "#<FFI::Type::Builtin::{} size={size} alignment={align}>",
                b.name()
            ))))
        }

        $($rest)*
        }
    };
}

builtin_type_class! {
    Type = zeo_abi::FFI_TYPE_CLASS, zeo_abi::OBJECT_CLASS;

    def "size"(recv) {
        Ok(RubyValue::Int(t_of(recv).builtin.layout().0 as i64))
    }
    def "alignment"(recv) {
        Ok(RubyValue::Int(t_of(recv).builtin.layout().1 as i64))
    }
}

// The DSL emits one `install_constants` per block, so the second class lives
// in its own module.
mod builtin {
    use super::*;

    builtin_type_class! {
        TypeBuiltin = zeo_abi::FFI_TYPE_BUILTIN_CLASS, zeo_abi::FFI_TYPE_CLASS;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_instance_per_canonical_type() {
        let addr = |v: &RubyValue| match v {
            RubyValue::Object(o) => Arc::as_ptr(o) as *const u8 as usize,
            _ => panic!("type_value answers objects"),
        };
        // `:char` IS `:int8` -- one object under two constant names.
        assert_eq!(addr(&type_value(Int8)), addr(&type_value(Int8)));
        // `:long` is NOT `:int64`, though both are eight signed bytes.
        assert_ne!(addr(&type_value(Long)), addr(&type_value(Int64)));
        assert_eq!(Long.layout(), Int64.layout());
    }

    #[test]
    fn layouts_are_lp64() {
        assert_eq!(Int32.layout(), (4, 4));
        assert_eq!(Int64.layout(), (8, 8));
        assert_eq!(Bool.layout(), (1, 1));
        assert_eq!(Float32.layout(), (4, 4));
        assert_eq!(Pointer.layout(), (8, 8));
        // The gem reports these as one byte rather than zero.
        assert_eq!(Void.layout(), (1, 1));
        assert_eq!(Varargs.layout(), (1, 1));
    }

    #[test]
    fn kind_resolution_accepts_types_and_symbols() {
        let v = type_value(Float64);
        assert!(matches!(kind_of_type_value(&v), Ok(FfiKind::F64)));
        let s = RubyValue::Symbol(crate::Symbol::intern("int"));
        assert!(matches!(kind_of_type_value(&s), Ok(FfiKind::I32)));
        // (a Varargs argument errors, but constructing the TypeError needs
        // the exception REGISTRY, absent in a unit test -- e2e covers it)
        assert!(is_varargs_type(&type_value(Varargs)));
    }
}
