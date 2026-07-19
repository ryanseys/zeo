//! The builtin method tables (Phase 17.1) -- one Rust module per Ruby core
//! class/module, mirroring CRuby's file-per-class source layout (string.c,
//! array.c, compar.c, ...). `struct`/`class`/`module` are Rust keywords, so
//! Struct follows the crate's existing `rproc.rs` precedent (`rstruct`) and
//! Class+Module share `class_module`.
//!
//! Architecture (the module-faithful placement the phase is named for):
//! every method is implemented ONCE, in the module/class that OWNS it in
//! CRuby -- `Comparable`'s 7 methods drive the receiver's own `<=>`,
//! `Enumerable`'s drive `each`, `Kernel` carries the universals `Object`
//! never owned -- and `send_value`/`send` find them by walking the
//! receiver's REAL ancestor chain (`dispatch::ancestors_of_value`), user
//! reopens first PER ANCESTOR. A table row therefore never needs to know
//! which concrete class it's serving.
//!
//! Every table function validates its own arity and argument types, raising
//! real rescuable `ArgumentError`/`TypeError` through
//! `dispatch::raise_error` -- CRuby's own behavior (`"a" + 1` is a
//! TypeError, not NoMethodError).

use crate::{ClassId, RubyValue, Signal};
use std::collections::HashMap;
use std::sync::LazyLock;

pub(crate) mod array;
pub(crate) mod basic_object;
pub(crate) mod class_module;
pub(crate) mod comparable;
pub(crate) mod complex;
pub(crate) mod enumerable;
pub(crate) mod enumerator;
pub(crate) mod exception;
pub(crate) mod float;
pub(crate) mod format;
pub(crate) mod hash;
pub(crate) mod integer;
pub(crate) mod io;
pub(crate) mod kernel;
pub(crate) mod lazy;
pub(crate) mod method_obj;
pub(crate) mod matchdata;
pub(crate) mod math;
pub(crate) mod dir;
pub(crate) mod encoding;
pub(crate) mod env;
pub(crate) mod file;
pub(crate) mod gc;
pub(crate) mod numeric;
pub(crate) mod object;
pub(crate) mod pack;
pub(crate) mod process;
pub(crate) mod time;
pub(crate) mod range;
pub(crate) mod rational;
pub(crate) mod random;
pub(crate) mod condition_variable;
pub(crate) mod set;
pub(crate) mod regexp;
pub(crate) mod fiber;
pub(crate) mod rproc;
pub(crate) mod string;
pub(crate) mod symbol;
pub(crate) mod thread;
pub(crate) mod value_subclass;

/// One builtin method: receiver (guaranteed by the table's ClassId keying
/// to be the right variant), positional args, optional block. Deliberately
/// the SAME shape as `ValueMethodFn` (Phase 16.3's reopen trampolines) --
/// one trampoline ABI for everything the MRO walk can find.
pub type BuiltinMethodFn =
    fn(&RubyValue, &[RubyValue], Option<RubyValue>) -> Result<RubyValue, Signal>;

/// The static ClassId -> method-table map. A plain match (rustc compiles it
/// to a jump table); `None` for user classes and for builtins with no table
/// yet. `Enumerable`/`Comparable` are NOT here -- their implementations
/// predate the table shape (`enumerable_send`/`comparable_send`) and are
/// special-cased as ancestors inside the MRO walk itself.
pub(crate) fn class_table(id: ClassId) -> Option<fn(&str) -> Option<BuiltinMethodFn>> {
    Some(match id {
        spinel_abi::INTEGER_CLASS => integer::lookup,
        spinel_abi::FLOAT_CLASS => float::lookup,
        spinel_abi::NUMERIC_CLASS => numeric::lookup,
        spinel_abi::RATIONAL_CLASS => rational::lookup,
        spinel_abi::COMPLEX_CLASS => complex::lookup,
        spinel_abi::STRING_CLASS => string::lookup,
        spinel_abi::SYMBOL_CLASS => symbol::lookup,
        spinel_abi::ARRAY_CLASS => array::lookup,
        spinel_abi::HASH_CLASS => hash::lookup,
        spinel_abi::RANGE_CLASS => range::lookup,
        spinel_abi::PROC_CLASS => rproc::lookup,
        spinel_abi::REGEXP_CLASS => regexp::lookup,
        spinel_abi::MATCH_DATA_CLASS => matchdata::lookup,
        spinel_abi::CLASS_CLASS => class_module::lookup_class,
        spinel_abi::MODULE_CLASS => class_module::lookup_module,
        spinel_abi::NIL_CLASS => object::lookup_nil,
        spinel_abi::TRUE_CLASS | spinel_abi::FALSE_CLASS => object::lookup_bool,
        spinel_abi::KERNEL_CLASS => kernel::lookup,
        spinel_abi::BASIC_OBJECT_CLASS => basic_object::lookup,
        spinel_abi::ENUMERATOR_CLASS => enumerator::lookup,
        spinel_abi::YIELDER_CLASS => enumerator::lookup_yielder,
        spinel_abi::IO_CLASS | spinel_abi::FILE_CLASS => io::lookup,
        spinel_abi::METHOD_CLASS => method_obj::lookup,
        spinel_abi::UNBOUND_METHOD_CLASS => method_obj::lookup_unbound,
        spinel_abi::FIBER_CLASS => fiber::lookup,
        spinel_abi::THREAD_CLASS => thread::lookup,
        spinel_abi::RANDOM_CLASS => random::lookup,
        spinel_abi::TIME_CLASS => time::lookup,
        spinel_abi::ENCODING_CLASS => encoding::lookup,
        spinel_abi::SET_CLASS => set::lookup,
        spinel_abi::LAZY_CLASS => lazy::lookup,
        spinel_abi::CONDITION_VARIABLE_CLASS => condition_variable::lookup,
        // In-tree `ext/` extensions with instances (modules like CGI/JSON have
        // none -- they appear only in `class_method_table`).
        #[cfg(feature = "ext-stringio")]
        spinel_abi::STRINGIO_CLASS => crate::ext::stringio::lookup,
        #[cfg(feature = "ext-strscan")]
        spinel_abi::STRING_SCANNER_CLASS => crate::ext::strscan::lookup,
        #[cfg(feature = "ext-digest")]
        spinel_abi::DIGEST_MD5_CLASS
        | spinel_abi::DIGEST_SHA1_CLASS
        | spinel_abi::DIGEST_SHA256_CLASS
        | spinel_abi::DIGEST_SHA512_CLASS => crate::ext::digest::lookup,
        #[cfg(feature = "ext-date")]
        spinel_abi::DATE_CLASS | spinel_abi::DATETIME_CLASS => crate::ext::date::lookup,
        #[cfg(feature = "ext-socket")]
        spinel_abi::SOCKET_CLASS => crate::ext::socket::lookup,
        _ => return None,
    })
}

/// The static ClassId -> CLASS-METHOD table map -- `class_table`'s
/// counterpart for methods invoked on the class/module VALUE itself
/// (`File.read`, `Time.now`, `Dir.pwd`, `Math.sqrt`), as opposed to on an
/// instance.
///
/// Needed because this runtime has no singleton-method tables: an instance
/// method table is keyed by the receiver's class, but the receiver of
/// `File.read` is `RubyValue::Class(FILE_CLASS)`, whose own class is
/// `Class` -- so the ordinary MRO walk looks at Class/Module and never at
/// File. `send_value_in` probes this table first for a `RubyValue::Class`
/// receiver, which is what `Math.sqrt` used to get via a hardcoded
/// `if *cid == MATH_CLASS` arm (and `GC` via a second one).
///
/// The `&RubyValue` a row receives is the CLASS VALUE itself, not an
/// instance -- rows generally ignore it (`Time.now` needs no receiver), but
/// it keeps the `BuiltinMethodFn` ABI uniform with `class_table`'s.
pub(crate) fn class_method_table(id: ClassId) -> Option<fn(&str) -> Option<BuiltinMethodFn>> {
    Some(match id {
        spinel_abi::INTEGER_CLASS => integer::lookup_class,
        spinel_abi::ARRAY_CLASS => array::lookup_class,
        spinel_abi::STRING_CLASS => string::lookup_class,
        spinel_abi::HASH_CLASS => hash::lookup_class,
        spinel_abi::PROC_CLASS => rproc::lookup_class,
        spinel_abi::REGEXP_CLASS => regexp::lookup_class,
        spinel_abi::FILE_CLASS => file::lookup_class,
        spinel_abi::DIR_CLASS => dir::lookup_class,
        spinel_abi::TIME_CLASS => time::lookup_class,
        spinel_abi::PROCESS_CLASS => process::lookup_class,
        spinel_abi::GC_CLASS => gc::lookup_class,
        spinel_abi::ENCODING_CLASS => encoding::lookup_class,
        spinel_abi::SET_CLASS => set::lookup_class,
        spinel_abi::COMPLEX_CLASS => complex::lookup_class,
        spinel_abi::RANDOM_CLASS => random::lookup_class,
        spinel_abi::CONDITION_VARIABLE_CLASS => condition_variable::lookup_class,
        spinel_abi::THREAD_CLASS => thread::lookup_class,
        // In-tree `ext/` extensions -- each behind its `ext-<name>` cargo
        // feature (see `ext/mod.rs`), so a feature-off build drops the arm.
        #[cfg(feature = "ext-base64")]
        spinel_abi::BASE64_MODULE => crate::ext::base64::lookup_class,
        #[cfg(feature = "ext-stringio")]
        spinel_abi::STRINGIO_CLASS => crate::ext::stringio::lookup_class,
        #[cfg(feature = "ext-strscan")]
        spinel_abi::STRING_SCANNER_CLASS => crate::ext::strscan::lookup_class,
        #[cfg(feature = "ext-cgi")]
        spinel_abi::CGI_MODULE => crate::ext::cgi::lookup_class,
        #[cfg(feature = "ext-digest")]
        spinel_abi::DIGEST_MD5_CLASS
        | spinel_abi::DIGEST_SHA1_CLASS
        | spinel_abi::DIGEST_SHA256_CLASS
        | spinel_abi::DIGEST_SHA512_CLASS => crate::ext::digest::lookup_class,
        #[cfg(feature = "ext-digest")]
        spinel_abi::DIGEST_MODULE => crate::ext::digest::lookup_module,
        #[cfg(feature = "ext-json")]
        spinel_abi::JSON_MODULE => crate::ext::json::lookup_class,
        #[cfg(feature = "ext-date")]
        spinel_abi::DATE_CLASS | spinel_abi::DATETIME_CLASS => crate::ext::date::lookup_class,
        #[cfg(feature = "ext-zlib")]
        spinel_abi::ZLIB_MODULE => crate::ext::zlib::lookup_class,
        #[cfg(feature = "ext-psych")]
        spinel_abi::PSYCH_MODULE | spinel_abi::YAML_MODULE => crate::ext::psych::lookup_class,
        #[cfg(feature = "ext-socket")]
        spinel_abi::SOCKET_CLASS => crate::ext::socket::lookup_class,
        #[cfg(feature = "ext-openssl")]
        spinel_abi::OPENSSL_MODULE => crate::ext::openssl::lookup_class,
        _ => return None,
    })
}

/// `class_table`'s reflection companion: the instance-method NAMES a builtin
/// class exposes (for `instance_methods`/`methods`). Mirrors `class_table`'s
/// arms exactly -- each `<mod>::lookup` has a paste-generated `<mod>::lookup_names`.
pub(crate) fn class_table_names(id: ClassId) -> &'static [&'static str] {
    match id {
        spinel_abi::INTEGER_CLASS => integer::lookup_names(),
        spinel_abi::FLOAT_CLASS => float::lookup_names(),
        spinel_abi::NUMERIC_CLASS => numeric::lookup_names(),
        spinel_abi::RATIONAL_CLASS => rational::lookup_names(),
        spinel_abi::COMPLEX_CLASS => complex::lookup_names(),
        spinel_abi::STRING_CLASS => string::lookup_names(),
        spinel_abi::SYMBOL_CLASS => symbol::lookup_names(),
        spinel_abi::ARRAY_CLASS => array::lookup_names(),
        spinel_abi::HASH_CLASS => hash::lookup_names(),
        spinel_abi::RANGE_CLASS => range::lookup_names(),
        spinel_abi::PROC_CLASS => rproc::lookup_names(),
        spinel_abi::REGEXP_CLASS => regexp::lookup_names(),
        spinel_abi::MATCH_DATA_CLASS => matchdata::lookup_names(),
        spinel_abi::CLASS_CLASS => class_module::lookup_class_names(),
        spinel_abi::MODULE_CLASS => class_module::lookup_module_names(),
        spinel_abi::NIL_CLASS => object::lookup_nil_names(),
        spinel_abi::TRUE_CLASS | spinel_abi::FALSE_CLASS => object::lookup_bool_names(),
        spinel_abi::KERNEL_CLASS => kernel::lookup_names(),
        spinel_abi::BASIC_OBJECT_CLASS => basic_object::lookup_names(),
        spinel_abi::ENUMERATOR_CLASS => enumerator::lookup_names(),
        spinel_abi::YIELDER_CLASS => enumerator::lookup_yielder_names(),
        spinel_abi::IO_CLASS | spinel_abi::FILE_CLASS => io::lookup_names(),
        spinel_abi::METHOD_CLASS => method_obj::lookup_names(),
        spinel_abi::UNBOUND_METHOD_CLASS => method_obj::lookup_unbound_names(),
        spinel_abi::FIBER_CLASS => fiber::lookup_names(),
        spinel_abi::THREAD_CLASS => thread::lookup_names(),
        spinel_abi::RANDOM_CLASS => random::lookup_names(),
        spinel_abi::TIME_CLASS => time::lookup_names(),
        spinel_abi::ENCODING_CLASS => encoding::lookup_names(),
        spinel_abi::SET_CLASS => set::lookup_names(),
        spinel_abi::LAZY_CLASS => lazy::lookup_names(),
        spinel_abi::CONDITION_VARIABLE_CLASS => condition_variable::lookup_names(),
        spinel_abi::ENUMERABLE_CLASS => enumerable::NAMES,
        spinel_abi::COMPARABLE_CLASS => comparable::NAMES,
        spinel_abi::MATH_CLASS => math::NAMES,
        #[cfg(feature = "ext-stringio")]
        spinel_abi::STRINGIO_CLASS => crate::ext::stringio::lookup_names(),
        #[cfg(feature = "ext-strscan")]
        spinel_abi::STRING_SCANNER_CLASS => crate::ext::strscan::lookup_names(),
        #[cfg(feature = "ext-digest")]
        spinel_abi::DIGEST_MD5_CLASS
        | spinel_abi::DIGEST_SHA1_CLASS
        | spinel_abi::DIGEST_SHA256_CLASS
        | spinel_abi::DIGEST_SHA512_CLASS => crate::ext::digest::lookup_names(),
        #[cfg(feature = "ext-date")]
        spinel_abi::DATE_CLASS | spinel_abi::DATETIME_CLASS => crate::ext::date::lookup_names(),
        #[cfg(feature = "ext-socket")]
        spinel_abi::SOCKET_CLASS => crate::ext::socket::lookup_names(),
        _ => &[],
    }
}

/// `class_method_table`'s reflection companion: the CLASS-method NAMES a
/// builtin exposes (for `SomeClass.singleton_methods` / `.methods`).
pub(crate) fn class_method_table_names(id: ClassId) -> &'static [&'static str] {
    match id {
        spinel_abi::INTEGER_CLASS => integer::lookup_class_names(),
        spinel_abi::ARRAY_CLASS => array::lookup_class_names(),
        spinel_abi::STRING_CLASS => string::lookup_class_names(),
        spinel_abi::HASH_CLASS => hash::lookup_class_names(),
        spinel_abi::PROC_CLASS => rproc::lookup_class_names(),
        spinel_abi::REGEXP_CLASS => regexp::lookup_class_names(),
        spinel_abi::FILE_CLASS => file::lookup_class_names(),
        spinel_abi::DIR_CLASS => dir::lookup_class_names(),
        spinel_abi::TIME_CLASS => time::lookup_class_names(),
        spinel_abi::PROCESS_CLASS => process::lookup_class_names(),
        spinel_abi::GC_CLASS => gc::lookup_class_names(),
        spinel_abi::ENCODING_CLASS => encoding::lookup_class_names(),
        spinel_abi::SET_CLASS => set::lookup_class_names(),
        spinel_abi::COMPLEX_CLASS => complex::lookup_class_names(),
        spinel_abi::CONDITION_VARIABLE_CLASS => condition_variable::lookup_class_names(),
        spinel_abi::THREAD_CLASS => thread::lookup_class_names(),
        #[cfg(feature = "ext-base64")]
        spinel_abi::BASE64_MODULE => crate::ext::base64::lookup_class_names(),
        #[cfg(feature = "ext-stringio")]
        spinel_abi::STRINGIO_CLASS => crate::ext::stringio::lookup_class_names(),
        #[cfg(feature = "ext-strscan")]
        spinel_abi::STRING_SCANNER_CLASS => crate::ext::strscan::lookup_class_names(),
        #[cfg(feature = "ext-cgi")]
        spinel_abi::CGI_MODULE => crate::ext::cgi::lookup_class_names(),
        #[cfg(feature = "ext-digest")]
        spinel_abi::DIGEST_MD5_CLASS
        | spinel_abi::DIGEST_SHA1_CLASS
        | spinel_abi::DIGEST_SHA256_CLASS
        | spinel_abi::DIGEST_SHA512_CLASS => crate::ext::digest::lookup_class_names(),
        _ => &[],
    }
}

/// Registry-FREE ancestor chains for the builtin classes, computed once
/// from the ABI's own declarative `superclass`/`includes` edges with the
/// same linearization rule as `analyze::mro::compute_ancestors` (self, then
/// includes reversed -- each recursively expanded -- then the parent's
/// chain; first occurrence wins). What lets the MRO walk (and this crate's
/// own unit tests) run without an installed `ClassRegistry`; a real
/// generated program's registry carries identical chains for builtins
/// (both derive from the one ABI table) plus richer ones for user classes.
pub(crate) fn fallback_ancestors(id: ClassId) -> &'static [ClassId] {
    static CHAINS: LazyLock<HashMap<u32, Vec<ClassId>>> = LazyLock::new(|| {
        fn edges(id: ClassId) -> (&'static [ClassId], Option<ClassId>) {
            if id == spinel_abi::OBJECT_CLASS {
                return (spinel_abi::OBJECT_INCLUDES, Some(spinel_abi::OBJECT_SUPERCLASS));
            }
            let b = &spinel_abi::BUILTINS[(id.0 as usize) - 1];
            (b.includes, b.superclass)
        }
        fn linearize(id: ClassId, out: &mut Vec<ClassId>) {
            if out.contains(&id) {
                return;
            }
            out.push(id);
            let (includes, parent) = edges(id);
            for &inc in includes.iter().rev() {
                linearize(inc, out);
            }
            if let Some(p) = parent {
                linearize(p, out);
            }
        }
        let mut chains = HashMap::new();
        for id in std::iter::once(spinel_abi::OBJECT_CLASS)
            .chain(spinel_abi::BUILTINS.iter().map(|b| b.id))
        {
            let mut chain = Vec::new();
            linearize(id, &mut chain);
            chains.insert(id.0, chain);
        }
        chains
    });
    CHAINS.get(&id.0).map_or(&[], |c| c.as_slice())
}

/// The Ruby class name of `v` for error messages -- registry name for user
/// objects, ABI name for everything else.
pub(crate) fn class_name_of(v: &RubyValue) -> String {
    crate::dispatch::class_name(v.class_id())
        .or_else(|| spinel_abi::builtin_name(v.class_id()).map(str::to_string))
        .unwrap_or_else(|| format!("#<Class:{}>", v.class_id().0))
}

/// Declares one Ruby class/module's method table: each row is a named
/// function (unit-testable, a real frame in backtraces) plus one generated
/// `lookup` match from Ruby method name(s) to it. Aliases share an
/// implementation via `"a" | "b"`. Rows spell an unused block parameter
/// `_block` like any Rust binding.
macro_rules! builtin_methods {
    (
        $lookup_vis:vis fn $lookup:ident;
        $( $($mname:literal)|+ => fn $fname:ident($recv:tt, $args:tt, $block:tt) $body:block )*
    ) => {
        $(
            pub(crate) fn $fname(
                $recv: &crate::RubyValue,
                $args: &[crate::RubyValue],
                $block: Option<crate::RubyValue>,
            ) -> Result<crate::RubyValue, crate::Signal> $body
        )*
        $lookup_vis fn $lookup(name: &str) -> Option<crate::builtins::BuiltinMethodFn> {
            match name {
                $( $($mname)|+ => Some($fname), )*
                _ => None,
            }
        }
        paste::paste! {
            /// Every method name this table exposes (each alias enumerated) --
            /// the reflection surface for `instance_methods`/`methods`. Derived
            /// from the same rows as the `lookup` above, so it can't drift.
            /// `allow(dead_code)`: a few tables (e.g. the `ENV` singleton, whose
            /// class is `Object`) never feed reflection, so their slice is unused.
            #[allow(dead_code)]
            $lookup_vis fn [<$lookup _names>]() -> &'static [&'static str] {
                &[ $( $($mname),+ ),* ]
            }
        }
    };
}
pub(crate) use builtin_methods;

/// CRuby's exact ArgumentError shapes for a fixed or ranged arity.
macro_rules! arity {
    ($args:expr, $n:literal) => {
        if $args.len() != $n {
            return Err(crate::dispatch::raise_error(
                "ArgumentError",
                format!("wrong number of arguments (given {}, expected {})", $args.len(), $n),
            ));
        }
    };
    ($args:expr, $lo:literal..=$hi:literal) => {
        if !($lo..=$hi).contains(&$args.len()) {
            return Err(crate::dispatch::raise_error(
                "ArgumentError",
                format!(
                    "wrong number of arguments (given {}, expected {}..{})",
                    $args.len(),
                    $lo,
                    $hi
                ),
            ));
        }
    };
}
pub(crate) use arity;

/// Receiver unwrappers -- the table's ClassId keying guarantees the
/// variant, so a mismatch is a dispatch bug, not a user error.
macro_rules! recv_str {
    ($recv:expr) => {
        match $recv {
            crate::RubyValue::Str(s) => s,
            _ => unreachable!("String table row dispatched on a non-String receiver"),
        }
    };
}
pub(crate) use recv_str;

macro_rules! recv_array {
    ($recv:expr) => {
        match $recv {
            crate::RubyValue::Array(a) => a,
            _ => unreachable!("Array table row dispatched on a non-Array receiver"),
        }
    };
}
pub(crate) use recv_array;

macro_rules! recv_hash {
    ($recv:expr) => {
        match $recv {
            crate::RubyValue::Hash(h) => h,
            _ => unreachable!("Hash table row dispatched on a non-Hash receiver"),
        }
    };
}
pub(crate) use recv_hash;

/// Argument coercion guards -- CRuby's exact TypeError shape.
macro_rules! arg_int {
    ($args:expr, $i:literal) => {
        match &$args[$i] {
            crate::RubyValue::Int(v) => *v,
            other => {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!(
                        "no implicit conversion of {} into Integer",
                        crate::builtins::class_name_of(other)
                    ),
                ))
            }
        }
    };
}
pub(crate) use arg_int;

macro_rules! arg_str {
    ($args:expr, $i:literal) => {
        match &$args[$i] {
            crate::RubyValue::Str(s) => s,
            other => {
                return Err(crate::dispatch::raise_error(
                    "TypeError",
                    format!(
                        "no implicit conversion of {} into String",
                        crate::builtins::class_name_of(other)
                    ),
                ))
            }
        }
    };
}
pub(crate) use arg_str;

/// The block -- or, blockless, an early return with the ENUMERATOR every
/// iteration method answers in real Ruby (Phase 17.2, retiring the
/// "would return an Enumerator (spike scope)" panics): the enumerator
/// captures `(recv, method-name, args)` and re-invokes the method when
/// iterated (`rb_enumeratorize`'s rule).
macro_rules! block_or_enum {
    ($recv:expr, $meth:expr, $args:expr, $block:expr) => {
        match $block {
            Some(crate::RubyValue::Proc(p)) => p,
            _ => {
                return Ok(crate::builtins::enumerator::enumerator_for($recv, $meth, $args))
            }
        }
    };
}
pub(crate) use block_or_enum;

/// The block, or CRuby's `LocalJumpError` (what a bare `yield` with no
/// block raises -- `5.tap` reproduces it, oracle-verified).
macro_rules! need_block {
    ($block:expr) => {
        match &$block {
            Some(crate::RubyValue::Proc(p)) => p.clone(),
            _ => {
                return Err(crate::dispatch::raise_error(
                    "LocalJumpError",
                    "no block given (yield)".to_string(),
                ))
            }
        }
    };
}
pub(crate) use need_block;

#[cfg(test)]
mod tests {
    use super::*;
    use spinel_abi::*;

    #[test]
    fn fallback_chains_match_the_oracle_hierarchy() {
        assert_eq!(
            fallback_ancestors(INTEGER_CLASS),
            &[
                INTEGER_CLASS,
                NUMERIC_CLASS,
                COMPARABLE_CLASS,
                OBJECT_CLASS,
                KERNEL_CLASS,
                BASIC_OBJECT_CLASS
            ]
        );
        assert_eq!(
            fallback_ancestors(STRING_CLASS),
            &[
                STRING_CLASS,
                COMPARABLE_CLASS,
                OBJECT_CLASS,
                KERNEL_CLASS,
                BASIC_OBJECT_CLASS
            ]
        );
        assert_eq!(
            fallback_ancestors(ARRAY_CLASS),
            &[
                ARRAY_CLASS,
                ENUMERABLE_CLASS,
                OBJECT_CLASS,
                KERNEL_CLASS,
                BASIC_OBJECT_CLASS
            ]
        );
        assert_eq!(
            fallback_ancestors(OBJECT_CLASS),
            &[OBJECT_CLASS, KERNEL_CLASS, BASIC_OBJECT_CLASS]
        );
        assert_eq!(fallback_ancestors(BASIC_OBJECT_CLASS), &[BASIC_OBJECT_CLASS]);
        assert_eq!(fallback_ancestors(KERNEL_CLASS), &[KERNEL_CLASS]);
        assert_eq!(
            fallback_ancestors(CLASS_CLASS),
            &[
                CLASS_CLASS,
                MODULE_CLASS,
                OBJECT_CLASS,
                KERNEL_CLASS,
                BASIC_OBJECT_CLASS
            ]
        );
        // Unknown (user) ids: empty, the registry's job.
        assert_eq!(fallback_ancestors(ClassId(999)), &[] as &[ClassId]);
    }

    #[test]
    fn class_table_covers_the_expected_ids() {
        assert!(class_table(STRING_CLASS).is_some());
        assert!(class_table(KERNEL_CLASS).is_some());
        assert!(class_table(BASIC_OBJECT_CLASS).is_some());
        assert!(class_table(ENUMERABLE_CLASS).is_none()); // special-cased in the walk
        assert!(class_table(ClassId(999)).is_none());
    }
}
