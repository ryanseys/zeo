//! The builtin method tables -- one Rust module per Ruby core
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

pub(crate) mod argf;
pub(crate) mod array;
pub(crate) mod basic_object;
pub(crate) mod class_module;
pub(crate) mod comparable;
pub(crate) mod complex;
pub(crate) mod condition_variable;
pub(crate) mod convert;
pub(crate) mod dir;
pub(crate) mod encoding;
pub(crate) mod enumerable;
pub(crate) mod formatter;
pub(crate) mod enumerator;
pub(crate) mod env;
pub(crate) mod exception;
pub(crate) mod fiber;
pub(crate) mod file;
pub(crate) mod float;
pub(crate) mod format;
pub(crate) mod gc;
pub(crate) mod hash;
pub(crate) mod integer;
pub(crate) mod io;
pub(crate) mod kernel;
pub(crate) mod lazy;
pub(crate) mod marshal;
pub(crate) mod matchdata;
pub(crate) mod math;
pub(crate) mod method_obj;
pub(crate) mod mutex;
pub(crate) mod numeric;
pub(crate) mod object;
pub(crate) mod pack;
pub(crate) mod process;
pub(crate) mod queue;
pub(crate) mod random;
pub(crate) mod range;
pub(crate) mod rational;
pub(crate) mod regexp;
pub(crate) mod rproc;
pub(crate) mod rstruct;
pub(crate) mod set;
pub(crate) mod signal;
pub(crate) mod stat;
pub(crate) mod string;
pub(crate) mod symbol;
pub(crate) mod thread;
pub(crate) mod thread_group;
pub(crate) mod time;
pub(crate) mod value_subclass;
pub(crate) mod warning;
pub(crate) mod weak;

/// One builtin method: receiver (guaranteed by the table's ClassId keying
/// to be the right variant), positional args, optional block. Deliberately
/// the SAME shape as `ValueMethodFn` (the reopen trampolines) --
/// one trampoline ABI for everything the MRO walk can find.
pub type BuiltinMethodFn =
    fn(&RubyValue, &[RubyValue], Option<RubyValue>) -> Result<RubyValue, Signal>;

/// One method surface (instance OR class): the same drift-free trio every
/// `builtin_methods!`/`ruby_class!` table derives from a single row set.
pub struct MethodTable {
    pub lookup: fn(&str) -> Option<BuiltinMethodFn>,
    pub names: fn() -> &'static [&'static str],
    pub arity: fn(&str) -> Option<i64>,
}

/// One builtin class/module's tables, registered by the `ruby_class!`/
/// `ruby_module!` macro and collected at link time into [`BUILTIN_TABLES`].
///
/// This is the data-driven replacement for the hand-written `ClassId`->lookup
/// `match` arms below: the routing fns (`class_table` etc.) consult the
/// registered table FIRST and fall back to the match only for classes not yet
/// migrated to the macro -- so a class is fully described by its own file the
/// moment it is registered here.
pub struct BuiltinClassTable {
    pub id: ClassId,
    /// Instance methods (`class_table`/`class_arity_table`/`class_table_names`).
    pub instance: Option<MethodTable>,
    /// Class/singleton methods (`class_method_table`/`class_method_table_names`).
    pub class: Option<MethodTable>,
    /// Seeds this class's constants at startup; `None` when it declares none.
    pub install_constants: Option<fn()>,
}

/// Every `ruby_class!`/`ruby_module!` class self-registers here; linkme
/// gathers them into one slice at final link (whether the runtime is linked as
/// an rlib or a dylib -- all elements live inside `zeo-rt`, so the slice is
/// self-contained either way).
#[linkme::distributed_slice]
pub static BUILTIN_TABLES: [BuiltinClassTable] = [..];

/// The registered tables indexed by `ClassId` for O(1) routing, built once
/// from the link-time-collected slice.
fn registered_table(id: ClassId) -> Option<&'static BuiltinClassTable> {
    static MAP: LazyLock<HashMap<u32, &'static BuiltinClassTable>> =
        LazyLock::new(|| BUILTIN_TABLES.iter().map(|t| (t.id.0, t)).collect());
    MAP.get(&id.0).copied()
}

/// The static ClassId -> method-table map. A plain match (rustc compiles it
/// to a jump table); `None` for user classes and for builtins with no table
/// yet. `Enumerable`/`Comparable` are ordinary rows here too -- the MRO
/// walk reaches them as ancestors of Array/Hash/Range and of any user class
/// that `include`s them, exactly like every other builtin module.
pub(crate) fn class_table(id: ClassId) -> Option<fn(&str) -> Option<BuiltinMethodFn>> {
    // A macro-registered class is fully described by its own table -- its match
    // arm below is deleted, so consult the registry first.
    if let Some(t) = registered_table(id) {
        return t.instance.as_ref().map(|m| m.lookup);
    }
    Some(match id {
        // INTEGER_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::FLOAT_CLASS => float::lookup,
        zeo_abi::NUMERIC_CLASS => numeric::lookup,
        // RATIONAL_CLASS migrated to ruby_class! -- served via registered_table.
        // COMPLEX_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::STRING_CLASS => string::lookup,
        // SYMBOL_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::ARRAY_CLASS => array::lookup,
        // HASH_CLASS migrated to ruby_class! -- served via registered_table.
        // RANGE_CLASS migrated to ruby_class! -- served via registered_table.
        // PROC_CLASS migrated to ruby_class! -- served via registered_table.
        // REGEXP_CLASS migrated to ruby_class! -- served via registered_table.
        // MATCH_DATA_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::CLASS_CLASS => class_module::lookup_class,
        zeo_abi::MODULE_CLASS => class_module::lookup_module,
        zeo_abi::NIL_CLASS => object::lookup_nil,
        zeo_abi::TRUE_CLASS | zeo_abi::FALSE_CLASS => object::lookup_bool,
        // KERNEL_CLASS migrated to ruby_module! -- served via registered_table.
        zeo_abi::BASIC_OBJECT_CLASS => basic_object::lookup,
        // ENUMERABLE_CLASS migrated to ruby_module! -- served via registered_table.
        // COMPARABLE_CLASS migrated to ruby_module! -- served via registered_table.
        // RANDOM_FORMATTER_MODULE migrated to ruby_module! -- served via registered_table.
        zeo_abi::ENUMERATOR_CLASS
        | zeo_abi::ENUMERATOR_CHAIN_CLASS
        | zeo_abi::ENUMERATOR_PRODUCT_CLASS => enumerator::lookup,
        zeo_abi::YIELDER_CLASS => enumerator::lookup_yielder,
        zeo_abi::IO_CLASS | zeo_abi::FILE_CLASS => io::lookup,
        // FILE_STAT_CLASS migrated to ruby_class! -- served via registered_table.
        // DIR_CLASS migrated to ruby_class! -- served via registered_table.
        // ARGF_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::METHOD_CLASS => method_obj::lookup,
        zeo_abi::UNBOUND_METHOD_CLASS => method_obj::lookup_unbound,
        zeo_abi::FIBER_CLASS => fiber::lookup,
        zeo_abi::THREAD_CLASS => thread::lookup,
        // THREAD_GROUP_CLASS migrated to ruby_class! -- served via registered_table.
        // RANDOM_CLASS migrated to ruby_class! -- served via registered_table.
        // TIME_CLASS migrated to ruby_class! -- served via registered_table.
        // ENCODING_CLASS migrated to ruby_class! -- served via registered_table.
        // SET_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::STRUCT_CLASS => rstruct::lookup,
        zeo_abi::DATA_CLASS => rstruct::lookup_data,
        // LAZY_CLASS migrated to ruby_class! -- served via registered_table.
        // CONDITION_VARIABLE_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::QUEUE_CLASS | zeo_abi::SIZED_QUEUE_CLASS => queue::lookup,
        // MUTEX_CLASS migrated to ruby_class! -- served via registered_table.
        // In-tree `ext/` extensions with instances (modules like CGI/JSON have
        // none -- they appear only in `class_method_table`).
        #[cfg(feature = "ext-stringio")]
        zeo_abi::STRINGIO_CLASS => crate::ext::stringio::lookup,
        #[cfg(feature = "ext-monitor")]
        zeo_abi::MONITOR_CLASS => crate::ext::monitor::lookup,
        #[cfg(feature = "ext-strscan")]
        zeo_abi::STRING_SCANNER_CLASS => crate::ext::strscan::lookup,
        #[cfg(feature = "ext-digest")]
        zeo_abi::DIGEST_MD5_CLASS
        | zeo_abi::DIGEST_SHA1_CLASS
        | zeo_abi::DIGEST_SHA256_CLASS
        | zeo_abi::DIGEST_SHA512_CLASS => crate::ext::digest::lookup,
        #[cfg(feature = "ext-date")]
        zeo_abi::DATE_CLASS | zeo_abi::DATETIME_CLASS => crate::ext::date::lookup,
        #[cfg(feature = "ext-socket")]
        zeo_abi::SOCKET_CLASS => crate::ext::socket::lookup,
        // TCPSocket has no own instance table -- it inherits IO's read/write via
        // the MRO (`TCPSocket < IO`). TCPServer adds accept/addr/listen/close.
        zeo_abi::TCPSERVER_CLASS => crate::ext::socket::lookup_tcpserver,
        #[cfg(feature = "ext-ffi")]
        zeo_abi::FFI_POINTER_CLASS | zeo_abi::FFI_MEMORY_POINTER_CLASS => crate::ext::ffi::lookup,
        #[cfg(feature = "ext-etc")]
        zeo_abi::ETC_PASSWD_CLASS => crate::ext::etc::passwd_lookup,
        #[cfg(feature = "ext-etc")]
        zeo_abi::ETC_GROUP_CLASS => crate::ext::etc::lookup_group,
        #[cfg(feature = "ext-pathname")]
        zeo_abi::PATHNAME_CLASS => crate::ext::pathname::lookup,
        _ => return None,
    })
}

/// `class_table`'s arity twin: ClassId -> the instance-method arity table
/// (`<lookup>_arity`, generated beside every `builtin_methods!` `lookup`).
/// `Method#arity` consults this for a builtin-receiver method object, walking
/// the receiver's ancestry so an inherited builtin resolves against its owner.
pub(crate) fn class_arity_table(id: ClassId) -> Option<fn(&str) -> Option<i64>> {
    if let Some(t) = registered_table(id) {
        return t.instance.as_ref().map(|m| m.arity);
    }
    Some(match id {
        // INTEGER_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::FLOAT_CLASS => float::lookup_arity,
        zeo_abi::NUMERIC_CLASS => numeric::lookup_arity,
        // RATIONAL_CLASS migrated to ruby_class! -- served via registered_table.
        // COMPLEX_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::STRING_CLASS => string::lookup_arity,
        // SYMBOL_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::ARRAY_CLASS => array::lookup_arity,
        // HASH_CLASS migrated to ruby_class! -- served via registered_table.
        // RANGE_CLASS migrated to ruby_class! -- served via registered_table.
        // PROC_CLASS migrated to ruby_class! -- served via registered_table.
        // REGEXP_CLASS migrated to ruby_class! -- served via registered_table.
        // MATCH_DATA_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::CLASS_CLASS => class_module::lookup_class_arity,
        zeo_abi::MODULE_CLASS => class_module::lookup_module_arity,
        zeo_abi::NIL_CLASS => object::lookup_nil_arity,
        zeo_abi::TRUE_CLASS | zeo_abi::FALSE_CLASS => object::lookup_bool_arity,
        // COMPARABLE_CLASS migrated to ruby_module! -- served via registered_table.
        // RANDOM_FORMATTER_MODULE migrated to ruby_module! -- served via registered_table.
        // ENUMERABLE_CLASS migrated to ruby_module! -- served via registered_table.
        // KERNEL_CLASS migrated to ruby_module! -- served via registered_table.
        zeo_abi::BASIC_OBJECT_CLASS => basic_object::lookup_arity,
        zeo_abi::ENUMERATOR_CLASS
        | zeo_abi::ENUMERATOR_CHAIN_CLASS
        | zeo_abi::ENUMERATOR_PRODUCT_CLASS => enumerator::lookup_arity,
        zeo_abi::YIELDER_CLASS => enumerator::lookup_yielder_arity,
        zeo_abi::IO_CLASS | zeo_abi::FILE_CLASS => io::lookup_arity,
        // FILE_STAT_CLASS migrated to ruby_class! -- served via registered_table.
        // DIR_CLASS migrated to ruby_class! -- served via registered_table.
        // ARGF_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::METHOD_CLASS => method_obj::lookup_arity,
        zeo_abi::UNBOUND_METHOD_CLASS => method_obj::lookup_unbound_arity,
        zeo_abi::FIBER_CLASS => fiber::lookup_arity,
        zeo_abi::THREAD_CLASS => thread::lookup_arity,
        // THREAD_GROUP_CLASS migrated to ruby_class! -- served via registered_table.
        // RANDOM_CLASS migrated to ruby_class! -- served via registered_table.
        // TIME_CLASS migrated to ruby_class! -- served via registered_table.
        // ENCODING_CLASS migrated to ruby_class! -- served via registered_table.
        // SET_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::STRUCT_CLASS => rstruct::lookup_arity,
        zeo_abi::DATA_CLASS => rstruct::lookup_data_arity,
        // LAZY_CLASS migrated to ruby_class! -- served via registered_table.
        // CONDITION_VARIABLE_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::QUEUE_CLASS | zeo_abi::SIZED_QUEUE_CLASS => queue::lookup_arity,
        // MUTEX_CLASS migrated to ruby_class! -- served via registered_table.
        #[cfg(feature = "ext-stringio")]
        zeo_abi::STRINGIO_CLASS => crate::ext::stringio::lookup_arity,
        #[cfg(feature = "ext-monitor")]
        zeo_abi::MONITOR_CLASS => crate::ext::monitor::lookup_arity,
        #[cfg(feature = "ext-strscan")]
        zeo_abi::STRING_SCANNER_CLASS => crate::ext::strscan::lookup_arity,
        #[cfg(feature = "ext-digest")]
        zeo_abi::DIGEST_MD5_CLASS
        | zeo_abi::DIGEST_SHA1_CLASS
        | zeo_abi::DIGEST_SHA256_CLASS
        | zeo_abi::DIGEST_SHA512_CLASS => crate::ext::digest::lookup_arity,
        #[cfg(feature = "ext-date")]
        zeo_abi::DATE_CLASS | zeo_abi::DATETIME_CLASS => crate::ext::date::lookup_arity,
        #[cfg(feature = "ext-socket")]
        zeo_abi::SOCKET_CLASS => crate::ext::socket::lookup_arity,
        zeo_abi::TCPSERVER_CLASS => crate::ext::socket::lookup_tcpserver_arity,
        #[cfg(feature = "ext-ffi")]
        zeo_abi::FFI_POINTER_CLASS | zeo_abi::FFI_MEMORY_POINTER_CLASS => {
            crate::ext::ffi::lookup_arity
        }
        #[cfg(feature = "ext-etc")]
        zeo_abi::ETC_PASSWD_CLASS => crate::ext::etc::lookup_passwd_arity,
        #[cfg(feature = "ext-etc")]
        zeo_abi::ETC_GROUP_CLASS => crate::ext::etc::lookup_group_arity,
        #[cfg(feature = "ext-pathname")]
        zeo_abi::PATHNAME_CLASS => crate::ext::pathname::lookup_arity,
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
    if let Some(t) = registered_table(id) {
        return t.class.as_ref().map(|m| m.lookup);
    }
    Some(match id {
        // INTEGER_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::ARRAY_CLASS => array::lookup_class,
        zeo_abi::STRING_CLASS => string::lookup_class,
        // HASH_CLASS migrated to ruby_class! -- served via registered_table.
        // PROC_CLASS migrated to ruby_class! -- served via registered_table.
        // REGEXP_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::FILE_CLASS => file::lookup_class,
        zeo_abi::FILE_TEST_MODULE => file::lookup_class,
        zeo_abi::IO_CLASS => io::lookup_class,
        // DIR_CLASS migrated to ruby_class! -- served via registered_table.
        // TIME_CLASS migrated to ruby_class! -- served via registered_table.
        // PROCESS_CLASS migrated to ruby_module! -- served via registered_table.
        // SIGNAL_MODULE migrated to ruby_module! -- served via registered_table.
        // WARNING_MODULE migrated to ruby_module! -- served via registered_table.
        // OBJECTSPACE_MODULE migrated to ruby_module! -- served via registered_table.
        // GC_CLASS migrated to ruby_module! -- served via registered_table.
        // ENCODING_CLASS migrated to ruby_class! -- served via registered_table.
        // SET_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::STRUCT_CLASS => rstruct::lookup_class,
        zeo_abi::DATA_CLASS => rstruct::lookup_class_data,
        // COMPLEX_CLASS migrated to ruby_class! -- served via registered_table.
        // RANDOM_CLASS migrated to ruby_class! -- served via registered_table.
        // MARSHAL_MODULE migrated to ruby_module! -- served via registered_table.
        zeo_abi::ENUMERATOR_CLASS => enumerator::lookup_class,
        // CONDITION_VARIABLE_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::THREAD_CLASS => thread::lookup_class,
        zeo_abi::FIBER_CLASS => fiber::lookup_class,
        zeo_abi::QUEUE_CLASS => queue::lookup_class,
        zeo_abi::SIZED_QUEUE_CLASS => queue::lookup_class_sized,
        // MUTEX_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::RACTOR_CLASS => crate::ractor::lookup_class,
        // In-tree `ext/` extensions -- each behind its `ext-<name>` cargo
        // feature (see `ext/mod.rs`), so a feature-off build drops the arm.
        #[cfg(feature = "ext-base64")]
        zeo_abi::BASE64_MODULE => crate::ext::base64::lookup_class,
        #[cfg(feature = "ext-etc")]
        zeo_abi::ETC_MODULE => crate::ext::etc::lookup_class,
        #[cfg(feature = "ext-pathname")]
        zeo_abi::PATHNAME_CLASS => crate::ext::pathname::lookup_class,
        #[cfg(feature = "ext-stringio")]
        zeo_abi::STRINGIO_CLASS => crate::ext::stringio::lookup_class,
        #[cfg(feature = "ext-monitor")]
        zeo_abi::MONITOR_CLASS => crate::ext::monitor::lookup_class,
        #[cfg(feature = "ext-strscan")]
        zeo_abi::STRING_SCANNER_CLASS => crate::ext::strscan::lookup_class,
        #[cfg(feature = "ext-cgi")]
        zeo_abi::CGI_MODULE => crate::ext::cgi::lookup_class,
        #[cfg(feature = "ext-digest")]
        zeo_abi::DIGEST_MD5_CLASS
        | zeo_abi::DIGEST_SHA1_CLASS
        | zeo_abi::DIGEST_SHA256_CLASS
        | zeo_abi::DIGEST_SHA512_CLASS => crate::ext::digest::lookup_class,
        #[cfg(feature = "ext-digest")]
        zeo_abi::DIGEST_MODULE => crate::ext::digest::lookup_module,
        #[cfg(feature = "ext-json")]
        zeo_abi::JSON_MODULE => crate::ext::json::lookup_class,
        #[cfg(feature = "ext-date")]
        zeo_abi::DATE_CLASS | zeo_abi::DATETIME_CLASS => crate::ext::date::lookup_class,
        #[cfg(feature = "ext-zlib")]
        zeo_abi::ZLIB_MODULE => crate::ext::zlib::lookup_class,
        #[cfg(feature = "ext-psych")]
        zeo_abi::PSYCH_MODULE | zeo_abi::YAML_MODULE => crate::ext::psych::lookup_class,
        #[cfg(feature = "ext-socket")]
        zeo_abi::SOCKET_CLASS => crate::ext::socket::lookup_class,
        zeo_abi::TCPSERVER_CLASS => crate::ext::socket::lookup_tcpserver_class,
        zeo_abi::TCPSOCKET_CLASS => crate::ext::socket::lookup_tcpsocket_class,
        #[cfg(feature = "ext-openssl")]
        zeo_abi::OPENSSL_MODULE => crate::ext::openssl::lookup_class,
        #[cfg(feature = "ext-ffi")]
        zeo_abi::FFI_POINTER_CLASS => crate::ext::ffi::lookup_class_pointer,
        #[cfg(feature = "ext-ffi")]
        zeo_abi::FFI_MEMORY_POINTER_CLASS => crate::ext::ffi::lookup_class_memory,
        _ => return None,
    })
}

/// `class_table`'s reflection companion: the instance-method NAMES a builtin
/// class exposes (for `instance_methods`/`methods`). Mirrors `class_table`'s
/// arms exactly -- each `<mod>::lookup` has a paste-generated `<mod>::lookup_names`.
pub(crate) fn class_table_names(id: ClassId) -> &'static [&'static str] {
    if let Some(t) = registered_table(id) {
        return t.instance.as_ref().map(|m| (m.names)()).unwrap_or(&[]);
    }
    match id {
        // INTEGER_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::FLOAT_CLASS => float::lookup_names(),
        zeo_abi::NUMERIC_CLASS => numeric::lookup_names(),
        // RATIONAL_CLASS migrated to ruby_class! -- served via registered_table.
        // COMPLEX_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::STRING_CLASS => string::lookup_names(),
        // SYMBOL_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::ARRAY_CLASS => array::lookup_names(),
        // HASH_CLASS migrated to ruby_class! -- served via registered_table.
        // RANGE_CLASS migrated to ruby_class! -- served via registered_table.
        // PROC_CLASS migrated to ruby_class! -- served via registered_table.
        // REGEXP_CLASS migrated to ruby_class! -- served via registered_table.
        // MATCH_DATA_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::CLASS_CLASS => class_module::lookup_class_names(),
        zeo_abi::MODULE_CLASS => class_module::lookup_module_names(),
        zeo_abi::NIL_CLASS => object::lookup_nil_names(),
        zeo_abi::TRUE_CLASS | zeo_abi::FALSE_CLASS => object::lookup_bool_names(),
        // KERNEL_CLASS migrated to ruby_module! -- served via registered_table.
        zeo_abi::BASIC_OBJECT_CLASS => basic_object::lookup_names(),
        zeo_abi::ENUMERATOR_CLASS
        | zeo_abi::ENUMERATOR_CHAIN_CLASS
        | zeo_abi::ENUMERATOR_PRODUCT_CLASS => enumerator::lookup_names(),
        zeo_abi::YIELDER_CLASS => enumerator::lookup_yielder_names(),
        zeo_abi::IO_CLASS | zeo_abi::FILE_CLASS => io::lookup_names(),
        // FILE_STAT_CLASS migrated to ruby_class! -- served via registered_table.
        // DIR_CLASS migrated to ruby_class! -- served via registered_table.
        // ARGF_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::METHOD_CLASS => method_obj::lookup_names(),
        zeo_abi::UNBOUND_METHOD_CLASS => method_obj::lookup_unbound_names(),
        zeo_abi::FIBER_CLASS => fiber::lookup_names(),
        zeo_abi::THREAD_CLASS => thread::lookup_names(),
        // THREAD_GROUP_CLASS migrated to ruby_class! -- served via registered_table.
        // RANDOM_CLASS migrated to ruby_class! -- served via registered_table.
        // TIME_CLASS migrated to ruby_class! -- served via registered_table.
        // ENCODING_CLASS migrated to ruby_class! -- served via registered_table.
        // SET_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::STRUCT_CLASS => rstruct::lookup_names(),
        zeo_abi::DATA_CLASS => rstruct::lookup_data_names(),
        // LAZY_CLASS migrated to ruby_class! -- served via registered_table.
        // CONDITION_VARIABLE_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::QUEUE_CLASS | zeo_abi::SIZED_QUEUE_CLASS => queue::lookup_names(),
        // MUTEX_CLASS migrated to ruby_class! -- served via registered_table.
        // ENUMERABLE_CLASS migrated to ruby_module! -- served via registered_table.
        // COMPARABLE_CLASS migrated to ruby_module! -- served via registered_table.
        // RANDOM_FORMATTER_MODULE migrated to ruby_module! -- served via registered_table.
        zeo_abi::MATH_CLASS => math::NAMES,
        #[cfg(feature = "ext-stringio")]
        zeo_abi::STRINGIO_CLASS => crate::ext::stringio::lookup_names(),
        #[cfg(feature = "ext-pathname")]
        zeo_abi::PATHNAME_CLASS => crate::ext::pathname::lookup_names(),
        #[cfg(feature = "ext-monitor")]
        zeo_abi::MONITOR_CLASS => crate::ext::monitor::lookup_names(),
        #[cfg(feature = "ext-strscan")]
        zeo_abi::STRING_SCANNER_CLASS => crate::ext::strscan::lookup_names(),
        #[cfg(feature = "ext-digest")]
        zeo_abi::DIGEST_MD5_CLASS
        | zeo_abi::DIGEST_SHA1_CLASS
        | zeo_abi::DIGEST_SHA256_CLASS
        | zeo_abi::DIGEST_SHA512_CLASS => crate::ext::digest::lookup_names(),
        #[cfg(feature = "ext-date")]
        zeo_abi::DATE_CLASS | zeo_abi::DATETIME_CLASS => crate::ext::date::lookup_names(),
        #[cfg(feature = "ext-socket")]
        zeo_abi::SOCKET_CLASS => crate::ext::socket::lookup_names(),
        zeo_abi::TCPSERVER_CLASS => crate::ext::socket::lookup_tcpserver_names(),
        #[cfg(feature = "ext-ffi")]
        zeo_abi::FFI_POINTER_CLASS | zeo_abi::FFI_MEMORY_POINTER_CLASS => {
            crate::ext::ffi::lookup_names()
        }
        _ => &[],
    }
}

/// `class_method_table`'s reflection companion: the CLASS-method NAMES a
/// builtin exposes (for `SomeClass.singleton_methods` / `.methods`).
pub(crate) fn class_method_table_names(id: ClassId) -> &'static [&'static str] {
    if let Some(t) = registered_table(id) {
        return t.class.as_ref().map(|m| (m.names)()).unwrap_or(&[]);
    }
    match id {
        // INTEGER_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::ARRAY_CLASS => array::lookup_class_names(),
        zeo_abi::STRING_CLASS => string::lookup_class_names(),
        // HASH_CLASS migrated to ruby_class! -- served via registered_table.
        // PROC_CLASS migrated to ruby_class! -- served via registered_table.
        // REGEXP_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::FILE_CLASS => file::lookup_class_names(),
        zeo_abi::FILE_TEST_MODULE => file::lookup_class_names(),
        zeo_abi::IO_CLASS => io::lookup_class_names(),
        // DIR_CLASS migrated to ruby_class! -- served via registered_table.
        // TIME_CLASS migrated to ruby_class! -- served via registered_table.
        // PROCESS_CLASS migrated to ruby_module! -- served via registered_table.
        // SIGNAL_MODULE migrated to ruby_module! -- served via registered_table.
        // WARNING_MODULE migrated to ruby_module! -- served via registered_table.
        // OBJECTSPACE_MODULE migrated to ruby_module! -- served via registered_table.
        // GC_CLASS migrated to ruby_module! -- served via registered_table.
        // ENCODING_CLASS migrated to ruby_class! -- served via registered_table.
        // SET_CLASS migrated to ruby_class! -- served via registered_table.
        // COMPLEX_CLASS migrated to ruby_class! -- served via registered_table.
        // MARSHAL_MODULE migrated to ruby_module! -- served via registered_table.
        zeo_abi::ENUMERATOR_CLASS => enumerator::lookup_class_names(),
        // CONDITION_VARIABLE_CLASS migrated to ruby_class! -- served via registered_table.
        zeo_abi::THREAD_CLASS => thread::lookup_class_names(),
        zeo_abi::FIBER_CLASS => fiber::lookup_class_names(),
        zeo_abi::QUEUE_CLASS => queue::lookup_class_names(),
        zeo_abi::SIZED_QUEUE_CLASS => queue::lookup_class_sized_names(),
        // MUTEX_CLASS migrated to ruby_class! -- served via registered_table.
        #[cfg(feature = "ext-base64")]
        zeo_abi::BASE64_MODULE => crate::ext::base64::lookup_class_names(),
        #[cfg(feature = "ext-etc")]
        zeo_abi::ETC_MODULE => crate::ext::etc::lookup_class_names(),
        #[cfg(feature = "ext-etc")]
        zeo_abi::ETC_PASSWD_CLASS => crate::ext::etc::passwd_names(),
        #[cfg(feature = "ext-etc")]
        zeo_abi::ETC_GROUP_CLASS => crate::ext::etc::lookup_group_names(),
        #[cfg(feature = "ext-pathname")]
        zeo_abi::PATHNAME_CLASS => crate::ext::pathname::lookup_class_names(),
        #[cfg(feature = "ext-stringio")]
        zeo_abi::STRINGIO_CLASS => crate::ext::stringio::lookup_class_names(),
        #[cfg(feature = "ext-monitor")]
        zeo_abi::MONITOR_CLASS => crate::ext::monitor::lookup_class_names(),
        #[cfg(feature = "ext-strscan")]
        zeo_abi::STRING_SCANNER_CLASS => crate::ext::strscan::lookup_class_names(),
        #[cfg(feature = "ext-cgi")]
        zeo_abi::CGI_MODULE => crate::ext::cgi::lookup_class_names(),
        #[cfg(feature = "ext-digest")]
        zeo_abi::DIGEST_MD5_CLASS
        | zeo_abi::DIGEST_SHA1_CLASS
        | zeo_abi::DIGEST_SHA256_CLASS
        | zeo_abi::DIGEST_SHA512_CLASS => crate::ext::digest::lookup_class_names(),
        #[cfg(feature = "ext-ffi")]
        zeo_abi::FFI_POINTER_CLASS => crate::ext::ffi::lookup_class_pointer_names(),
        #[cfg(feature = "ext-ffi")]
        zeo_abi::FFI_MEMORY_POINTER_CLASS => crate::ext::ffi::lookup_class_memory_names(),
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
            if id == zeo_abi::OBJECT_CLASS {
                return (zeo_abi::OBJECT_INCLUDES, Some(zeo_abi::OBJECT_SUPERCLASS));
            }
            let b = &zeo_abi::BUILTINS[(id.0 as usize) - 1];
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
        for id in
            std::iter::once(zeo_abi::OBJECT_CLASS).chain(zeo_abi::BUILTINS.iter().map(|b| b.id))
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
        .or_else(|| zeo_abi::builtin_name(v.class_id()).map(str::to_string))
        .unwrap_or_else(|| format!("#<Class:{}>", v.class_id().0))
}

/// CRuby's `rb_check_frozen` over any VALUE receiver: the standard
/// `can't modify frozen <Class>: <inspect>` FrozenError (with the receiver
/// detail attached, backing `FrozenError#receiver`), raised BEFORE the
/// caller mutates anything. The value-level counterpart to the per-file
/// collection guards (`guard_str_frozen`/`guard_hash_frozen`/array's
/// `check_frozen`), which keep their handle-shaped signatures.
pub(crate) fn check_frozen(recv: &RubyValue) -> Result<(), crate::Signal> {
    if recv.is_frozen() {
        return Err(crate::dispatch::raise_error_details(
            "FrozenError",
            format!(
                "can't modify frozen {}: {}",
                class_name_of(recv),
                recv.inspect_string()
            ),
            &[("receiver", recv.clone())],
        ));
    }
    Ok(())
}

/// The name CRuby uses for `v` in a coercion `TypeError` -- "no implicit
/// conversion of X into Y" / "can't convert X into Y". CRuby renders `nil`,
/// `true`, and `false` as those literals rather than their class names
/// (`NilClass`/`TrueClass`/`FalseClass`); every other object uses its class
/// name. Use this, not `class_name_of`, when building those messages.
pub(crate) fn convert_name_of(v: &RubyValue) -> String {
    match v {
        RubyValue::Nil => "nil".to_string(),
        RubyValue::Bool(true) => "true".to_string(),
        RubyValue::Bool(false) => "false".to_string(),
        _ => class_name_of(v),
    }
}

/// Declares one Ruby class/module's method table: each row is a named
/// function (unit-testable, a real frame in backtraces) plus one generated
/// `lookup` match from Ruby method name(s) to it. Aliases share an
/// implementation via `"a" | "b"`. Rows spell an unused block parameter
/// `_block` like any Rust binding.
macro_rules! builtin_methods {
    (
        $lookup_vis:vis fn $lookup:ident;
        $( $($mname:literal $([$arity:literal])?)|+ => fn $fname:ident($recv:tt, $args:tt, $block:tt) $body:block )*
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
        pastey::paste! {
            /// Every method name this table exposes (each alias enumerated) --
            /// the reflection surface for `instance_methods`/`methods`. Derived
            /// from the same rows as the `lookup` above, so it can't drift.
            /// `allow(dead_code)`: a few tables (e.g. the `ENV` singleton, whose
            /// class is `Object`) never feed reflection, so their slice is unused.
            #[allow(dead_code)]
            $lookup_vis fn [<$lookup _names>]() -> &'static [&'static str] {
                &[ $( $($mname),+ ),* ]
            }
            /// `Method#arity` for each method this table defines -- CRuby's
            /// per-method argc, DECLARED at the definition site as an optional
            /// `[n]` after each NAME literal (per-name, since aliases can differ:
            /// `Array#<<` is 1 but `#push` is -1). Mirrors `rb_define_method`'s
            /// argc column; an un-annotated name defaults to `-1`, CRuby's
            /// variadic-cfunc arity. `None` when this table does not define
            /// `name`.
            #[allow(dead_code)]
            $lookup_vis fn [<$lookup _arity>](name: &str) -> Option<i64> {
                match name {
                    $( $( $mname => Some({
                        let _a: i64 = -1;
                        $( let _a: i64 = $arity; )?
                        _a
                    }), )+ )*
                    _ => None,
                }
            }
        }
    };
}
pub(crate) use builtin_methods;

/// The typed error constructors: `type_error!("no implicit conversion...")`
/// over `raise_error("TypeError", format!(...))`, so the class name is spelled
/// once here (never typo-able per site) and call sites read as what they
/// raise. Each takes `format!` arguments and yields a `Signal` -- wrap in
/// `Err(...)` exactly as with `raise_error`. Classes raised from only one
/// site (Errno::*, ext-specific classes) stay on `raise_error` directly.
/// (Written flat rather than macro-generated: `$$` meta-variable escaping is
/// still unstable.)
macro_rules! type_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("TypeError", format!($($fmt)*)) };
}
macro_rules! arg_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("ArgumentError", format!($($fmt)*)) };
}
macro_rules! name_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("NameError", format!($($fmt)*)) };
}
macro_rules! index_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("IndexError", format!($($fmt)*)) };
}
macro_rules! range_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("RangeError", format!($($fmt)*)) };
}
macro_rules! runtime_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("RuntimeError", format!($($fmt)*)) };
}
macro_rules! frozen_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("FrozenError", format!($($fmt)*)) };
}
macro_rules! io_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("IOError", format!($($fmt)*)) };
}
macro_rules! eof_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("EOFError", format!($($fmt)*)) };
}
macro_rules! thread_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("ThreadError", format!($($fmt)*)) };
}
macro_rules! regexp_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("RegexpError", format!($($fmt)*)) };
}
macro_rules! local_jump_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("LocalJumpError", format!($($fmt)*)) };
}
macro_rules! float_domain_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("FloatDomainError", format!($($fmt)*)) };
}
macro_rules! not_impl_error {
    ($($fmt:tt)*) => { crate::dispatch::raise_error("NotImplementedError", format!($($fmt)*)) };
}
pub(crate) use {
    arg_error, eof_error, float_domain_error, frozen_error, index_error, io_error,
    local_jump_error, name_error, not_impl_error, range_error, regexp_error, runtime_error,
    thread_error, type_error,
};

/// CRuby's exact ArgumentError shapes for a fixed or ranged arity.
macro_rules! arity {
    ($args:expr_2021, $n:literal) => {
        if $args.len() != $n {
            return Err(crate::dispatch::raise_error(
                "ArgumentError",
                format!(
                    "wrong number of arguments (given {}, expected {})",
                    $args.len(),
                    $n
                ),
            ));
        }
    };
    ($args:expr_2021, $lo:literal..=$hi:literal) => {
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
    ($recv:expr_2021) => {
        match $recv {
            crate::RubyValue::Str(s) => s,
            _ => unreachable!("String table row dispatched on a non-String receiver"),
        }
    };
}
pub(crate) use recv_str;

macro_rules! recv_array {
    ($recv:expr_2021) => {
        match $recv {
            crate::RubyValue::Array(a) => a,
            _ => unreachable!("Array table row dispatched on a non-Array receiver"),
        }
    };
}
pub(crate) use recv_array;

macro_rules! recv_hash {
    ($recv:expr_2021) => {
        match $recv {
            crate::RubyValue::Hash(h) => h,
            _ => unreachable!("Hash table row dispatched on a non-Hash receiver"),
        }
    };
}
pub(crate) use recv_hash;

/// Argument coercion guards -- CRuby's exact TypeError shape.
/// Argument `$i` as an `i64` through the full implicit-conversion protocol
/// (`convert::to_index`): `Int` fast path, `to_int` duck types accepted,
/// CRuby's TypeError for the rest and RangeError for bignum-range answers.
macro_rules! arg_int {
    ($args:expr_2021, $i:literal) => {
        match &$args[$i] {
            crate::RubyValue::Int(v) => *v,
            other => crate::builtins::convert::to_index(other)?,
        }
    };
}
pub(crate) use arg_int;

/// Argument `$i` as a string handle through the protocol (`convert::to_rstr`):
/// `Str` fast path (an `Arc` bump), `to_str` duck types accepted, CRuby's
/// TypeError for the rest.
macro_rules! arg_str {
    ($args:expr_2021, $i:literal) => {
        match &$args[$i] {
            crate::RubyValue::Str(s) => s.clone(),
            other => crate::builtins::convert::to_rstr(other)?,
        }
    };
}
pub(crate) use arg_str;

/// The block -- or, blockless, an early return with the ENUMERATOR every
/// iteration method answers in real Ruby (retiring the
/// "would return an Enumerator" panics): the enumerator
/// captures `(recv, method-name, args)` and re-invokes the method when
/// iterated (`rb_enumeratorize`'s rule).
macro_rules! block_or_enum {
    ($recv:expr_2021, $meth:expr_2021, $args:expr_2021, $block:expr_2021) => {
        match $block {
            Some(crate::RubyValue::Proc(p)) => p,
            _ => {
                return Ok(crate::builtins::enumerator::enumerator_for(
                    $recv, $meth, $args,
                ))
            }
        }
    };
}
pub(crate) use block_or_enum;

/// The block, or CRuby's `LocalJumpError` (what a bare `yield` with no
/// block raises -- `5.tap` reproduces it, oracle-verified).
macro_rules! need_block {
    ($block:expr_2021) => {
        match &$block {
            Some(crate::RubyValue::Proc(p)) => p.clone(),
            _ => return Err(crate::dispatch::raise_no_block_yield()),
        }
    };
}
pub(crate) use need_block;

#[cfg(test)]
mod tests {
    use super::*;
    use zeo_abi::*;

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
        assert_eq!(
            fallback_ancestors(BASIC_OBJECT_CLASS),
            &[BASIC_OBJECT_CLASS]
        );
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
        assert!(class_table(ENUMERABLE_CLASS).is_some());
        assert!(class_table(ClassId(999)).is_none());
    }
}
