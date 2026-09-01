//! The reserved built-in class table ([`BUILTINS`]) and its accessors.

use crate::*;

/// Builtins that are NOT payload roots, because their subclasses are a
/// DIFFERENT native shape -- each has its own machinery, and routing one
/// through the payload bridge would break it.
///
/// This is the whole of the exception to "every built-in class can be
/// subclassed". Exceptions need no row: they live in [`EXCEPTION_CLASSES`],
/// not [`BUILTINS`], so the table lookup in [`is_payload_root`] already
/// misses them and `is_exception_backed` keeps them.
pub const NOT_PAYLOAD_ROOTS: &[ClassId] = &[
    // The roots themselves: a plain user class is a GENERATED STRUCT, which
    // is the whole point of not wrapping anything.
    OBJECT_CLASS,
    BASIC_OBJECT_CLASS,
    // No per-value dispatch to hang a payload on. A `class X < Module` is a
    // module FACTORY whose instances are real runtime module ids
    // (`Compiler::is_module_subclass`); `Class` has no shape at all.
    MODULE_CLASS,
    CLASS_CLASS,
    // The immediates and the allocator-undefined pair: CRuby accepts the
    // DEFINITION and has no instances (`Compiler::is_immediate_subclass`).
    INTEGER_CLASS,
    FLOAT_CLASS,
    SYMBOL_CLASS,
    NIL_CLASS,
    TRUE_CLASS,
    FALSE_CLASS,
    NUMERIC_CLASS,
    BIGDECIMAL_CLASS,
    METHOD_CLASS,
    BINDING_CLASS,
    ENCODING_CLASS,
    RATIONAL_CLASS,
    MATCH_DATA_CLASS,
    // Generated-struct shapes of their own: `Struct`/`Data` subclasses are
    // ordinary ivar objects, and an `FFI::Struct`/`FFI::Union` subclass is
    // one whose accessors are synthesized from its `layout`.
    STRUCT_CLASS,
    DATA_CLASS,
    FFI_STRUCT_CLASS,
    FFI_UNION_CLASS,
    // A NAMESPACE slot whose class body is Ruby (see `WEAKREF_CLASS`): there is
    // no native payload to wrap, and its subclasses are ordinary objects.
    WEAKREF_CLASS,
    // Constructors that already honour the RECEIVER class, so the subclass
    // IS the native type rather than a wrapper around one.
    WEAKMAP_CLASS,
    DATE_CLASS,
    DATETIME_CLASS,
    // The class rides IN the proc, so a subclass instance is still a
    // `RubyValue::Proc` and every call-site fast path keeps working.
    PROC_CLASS,
];

/// Whether `id` is a value-builtin payload root: a built-in CLASS whose
/// user subclass is a generic `ValueSubclass` wrapping the native value.
///
/// Every built-in class is one unless [`NOT_PAYLOAD_ROOTS`] says otherwise.
/// It used to be the other way round -- an opt-in allowlist -- which meant a
/// gem subclassing any native class zeo had not thought of got a compile
/// error naming zeo rather than a program. The two halves a root needs are
/// both generic now: the runtime's `empty_payload` falls back to `nil` (the
/// `File` shape -- the subclass's own `initialize` seats the real payload
/// through `super`), and `construct_root_payload` answers rather than
/// panicking when a root has no `new` row. A hand-written `empty_payload`
/// arm is now only an IMPROVEMENT on that default, not a prerequisite.
pub fn is_payload_root(id: ClassId) -> bool {
    if NOT_PAYLOAD_ROOTS.contains(&id) {
        return false;
    }
    builtin_class(id).is_some_and(|b| !b.is_module)
}

/// The [`BUILTINS`] row for `id` (ids are contiguous from 1), or `None` for
/// a user class, an exception class, or `Object`.
pub fn builtin_class(id: ClassId) -> Option<&'static BuiltinClass> {
    BUILTINS
        .get((id.0 as usize).wrapping_sub(1))
        .filter(|b| b.id == id)
}

/// The payload root `id` inherits from, `id` itself included -- the
/// compile-time twin of the runtime's `value_subclass::value_root_of`, over
/// the DECLARED builtin superclass edges. A builtin between a root and the
/// user's class is subclassable through that root: `class Handle <
/// FFI::AutoPointer` is a Pointer payload, because `AutoPointer < Pointer`
/// and `Pointer` is the root.
pub fn payload_root_of(id: ClassId) -> Option<ClassId> {
    let mut at = id;
    loop {
        if is_payload_root(at) {
            return Some(at);
        }
        at = builtin_class(at)?.superclass?;
    }
}

/// Every reserved built-in class/module except `Object` (see
/// [`OBJECT_CLASS`]), in id order -- ids are contiguous from 1 by
/// construction (asserted by the unit test below), which is what lets the
/// compiler seed its class arena by pushing these in order. Superclass
/// edges may point FORWARD in the table (`Integer(1)` -> `Numeric(26)`);
/// consumers store the edge and linearize later.
pub const BUILTINS: &[BuiltinClass] = &[
    BuiltinClass {
        id: INTEGER_CLASS,
        name: "Integer",
        is_module: false,
        superclass: Some(NUMERIC_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: FLOAT_CLASS,
        name: "Float",
        is_module: false,
        superclass: Some(NUMERIC_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: STRING_CLASS,
        name: "String",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[COMPARABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: SYMBOL_CLASS,
        name: "Symbol",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[COMPARABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: ARRAY_CLASS,
        name: "Array",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[ENUMERABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: HASH_CLASS,
        name: "Hash",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[ENUMERABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: RANGE_CLASS,
        name: "Range",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[ENUMERABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: NIL_CLASS,
        name: "NilClass",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: TRUE_CLASS,
        name: "TrueClass",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: FALSE_CLASS,
        name: "FalseClass",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: PROC_CLASS,
        name: "Proc",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: REGEXP_CLASS,
        name: "Regexp",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: MATCH_DATA_CLASS,
        name: "MatchData",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: FIBER_CLASS,
        name: "Fiber",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: THREAD_CLASS,
        name: "Thread",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: MUTEX_CLASS,
        name: "Thread::Mutex",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: QUEUE_CLASS,
        name: "Thread::Queue",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: RACTOR_CLASS,
        name: "Ractor",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: ENUMERABLE_CLASS,
        name: "Enumerable",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: CLASS_CLASS,
        name: "Class",
        is_module: false,
        superclass: Some(MODULE_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: MODULE_CLASS,
        name: "Module",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: COMPARABLE_CLASS,
        name: "Comparable",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: ENUMERATOR_CLASS,
        name: "Enumerator",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[ENUMERABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: BASIC_OBJECT_CLASS,
        name: "BasicObject",
        is_module: false,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: KERNEL_CLASS,
        name: "Kernel",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: NUMERIC_CLASS,
        name: "Numeric",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[COMPARABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: RATIONAL_CLASS,
        name: "Rational",
        is_module: false,
        superclass: Some(NUMERIC_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: COMPLEX_CLASS,
        name: "Complex",
        is_module: false,
        superclass: Some(NUMERIC_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: MATH_CLASS,
        name: "Math",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: STRUCT_CLASS,
        name: "Struct",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[ENUMERABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: YIELDER_CLASS,
        name: "Enumerator::Yielder",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: GC_CLASS,
        name: "GC",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: IO_CLASS,
        name: "IO",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        // Reverse of the resolution order, as `include A; include B` is:
        // CRuby's `IO.ancestors` is `[IO, File::Constants, Enumerable, ...]`.
        includes: &[ENUMERABLE_CLASS, FILE_CONSTANTS_MODULE],
        feature: None,
    },
    BuiltinClass {
        id: METHOD_CLASS,
        name: "Method",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: FILE_CLASS,
        name: "File",
        is_module: false,
        superclass: Some(IO_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: DIR_CLASS,
        name: "Dir",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[ENUMERABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: TIME_CLASS,
        name: "Time",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[COMPARABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: PROCESS_CLASS,
        name: "Process",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: FILE_STAT_CLASS,
        name: "File::Stat",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[COMPARABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: ENCODING_CLASS,
        name: "Encoding",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: DATA_CLASS,
        name: "Data",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: SET_CLASS,
        name: "Set",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[ENUMERABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: LAZY_CLASS,
        name: "Enumerator::Lazy",
        is_module: false,
        // CRuby's real hierarchy: `Enumerator::Lazy < Enumerator`, which
        // already includes Enumerable -- a direct include here would put
        // Enumerable AHEAD of Enumerator in the linearized ancestors.
        superclass: Some(ENUMERATOR_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: CONDITION_VARIABLE_CLASS,
        name: "Thread::ConditionVariable",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: UNBOUND_METHOD_CLASS,
        name: "UnboundMethod",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: BASE64_MODULE,
        name: "Base64",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("base64"),
    },
    // In-tree `ext/` extensions -- CRuby's ext/ model. Each is require-gated
    // (its constant is invisible until its `require` fires) AND compile-gated
    // by a per-extension cargo feature on `zeo-rt` (see that crate's
    // `[features]` and `ext/mod.rs`). Some carry real implementations, others
    // are scaffolded (a couple methods, the rest `todo!`) -- see
    // `docs/EXTENSIONS.md` for the per-extension status.
    BuiltinClass {
        id: STRINGIO_CLASS,
        name: "StringIO",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[ENUMERABLE_CLASS],
        feature: Some("stringio"),
    },
    BuiltinClass {
        id: STRING_SCANNER_CLASS,
        name: "StringScanner",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("strscan"),
    },
    BuiltinClass {
        // CRuby's CGI is a CLASS (`CGI.new(...)` is the whole CGI API), and
        // cgi/escape.rb opens it as one. The id keeps its `_MODULE` name so
        // every reference to it stays put.
        id: CGI_MODULE,
        name: "CGI",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        // And it EXTENDS the same module -- see `BUILTIN_EXTENDS`. The include
        // is what makes `CGI.new(...).escapeHTML` work; the extend is what
        // makes `CGI.escapeHTML` work. CRuby does both.
        includes: &[CGI_ESCAPE_MODULE],
        feature: Some("cgi/escape"),
    },
    BuiltinClass {
        id: DIGEST_MODULE,
        name: "Digest",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("digest"),
    },
    BuiltinClass {
        id: DIGEST_MD5_CLASS,
        name: "Digest::MD5",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("digest"),
    },
    BuiltinClass {
        id: DIGEST_SHA1_CLASS,
        name: "Digest::SHA1",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("digest"),
    },
    BuiltinClass {
        id: DIGEST_SHA256_CLASS,
        name: "Digest::SHA256",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("digest"),
    },
    BuiltinClass {
        id: DIGEST_SHA512_CLASS,
        name: "Digest::SHA512",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("digest"),
    },
    BuiltinClass {
        id: JSON_MODULE,
        name: "JSON",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("json"),
    },
    BuiltinClass {
        id: DATE_CLASS,
        name: "Date",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[COMPARABLE_CLASS],
        feature: Some("date"),
    },
    BuiltinClass {
        id: DATETIME_CLASS,
        name: "DateTime",
        is_module: false,
        superclass: Some(DATE_CLASS),
        includes: &[],
        feature: Some("date"),
    },
    BuiltinClass {
        id: ZLIB_MODULE,
        name: "Zlib",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("zlib"),
    },
    BuiltinClass {
        id: PSYCH_MODULE,
        name: "Psych",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("psych"),
    },
    BuiltinClass {
        id: SOCKET_CLASS,
        name: "Socket",
        is_module: false,
        superclass: Some(BASIC_SOCKET_CLASS),
        includes: &[],
        feature: Some("socket"),
    },
    BuiltinClass {
        id: OPENSSL_MODULE,
        name: "OpenSSL",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("openssl"),
    },
    // `YAML` is `Psych` under Ruby's `yaml.rb` (`YAML = Psych`); it shares the
    // `psych` feature so `require "yaml"` (canonicalized to `psych` by the
    // loader) makes both constants resolve, and both dispatch to `ext::psych`.
    BuiltinClass {
        id: YAML_MODULE,
        name: "YAML",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("psych"),
    },
    BuiltinClass {
        id: RANDOM_CLASS,
        name: "Random",
        is_module: false,
        superclass: Some(RANDOM_BASE_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: SIZED_QUEUE_CLASS,
        name: "Thread::SizedQueue",
        is_module: false,
        superclass: Some(QUEUE_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: MARSHAL_MODULE,
        name: "Marshal",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: MONITOR_CLASS,
        name: "Monitor",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("monitor"),
    },
    BuiltinClass {
        id: PROCESS_STATUS_CLASS,
        name: "Process::Status",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    // The real `ffi` gem, require-gated on "ffi". The `FFI` module row
    // MUST precede its nested classes (nested-constant resolution walks parent
    // first). `MemoryPointer < Pointer` so it inherits Pointer's accessors.
    BuiltinClass {
        id: FFI_MODULE,
        name: "FFI",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("ffi"),
    },
    BuiltinClass {
        id: FFI_POINTER_CLASS,
        name: "FFI::Pointer",
        is_module: false,
        // The gem's hierarchy: `Pointer < AbstractMemory` (a forward edge --
        // the `AbstractMemory` row lives with the other late FFI ids).
        superclass: Some(FFI_ABSTRACT_MEMORY_CLASS),
        includes: &[],
        feature: Some("ffi"),
    },
    BuiltinClass {
        id: FFI_MEMORY_POINTER_CLASS,
        name: "FFI::MemoryPointer",
        is_module: false,
        superclass: Some(FFI_POINTER_CLASS),
        includes: &[],
        feature: Some("ffi"),
    },
    BuiltinClass {
        id: FFI_STRUCT_CLASS,
        name: "FFI::Struct",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("ffi"),
    },
    BuiltinClass {
        id: SIGNAL_MODULE,
        name: "Signal",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: FILE_TEST_MODULE,
        name: "FileTest",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: ARGF_CLASS,
        name: "ARGF.class",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[ENUMERABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: ENUMERATOR_CHAIN_CLASS,
        name: "Enumerator::Chain",
        is_module: false,
        superclass: Some(ENUMERATOR_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: ENUMERATOR_PRODUCT_CLASS,
        name: "Enumerator::Product",
        is_module: false,
        superclass: Some(ENUMERATOR_CLASS),
        includes: &[],
        feature: None,
    },
    // A real `Struct` subclass, as `rb_struct_define` makes it in CRuby --
    // which is where its `to_a`/`to_h`/`==`/`each`/`[]`/`dig`/`deconstruct`
    // come from. `Enumerable` rides in with `Struct`, so it is not listed.
    BuiltinClass {
        id: PROCESS_TMS_CLASS,
        name: "Process::Tms",
        is_module: false,
        superclass: Some(STRUCT_CLASS),
        includes: &[],
        feature: None,
    },
    // `require "socket"`. TCPSocket < IPSocket (inherits BasicSocket's raw-fd
    // ops + IO's read/write/gets); TCPServer < TCPSocket (adds accept/addr).
    // The rest of the socket hierarchy (BasicSocket, IPSocket, ...) is appended
    // at the very end of the table (ids 90+).
    BuiltinClass {
        id: TCPSOCKET_CLASS,
        name: "TCPSocket",
        is_module: false,
        superclass: Some(IP_SOCKET_CLASS),
        includes: &[],
        feature: Some("socket"),
    },
    BuiltinClass {
        id: TCPSERVER_CLASS,
        name: "TCPServer",
        is_module: false,
        superclass: Some(TCPSOCKET_CLASS),
        includes: &[],
        feature: Some("socket"),
    },
    BuiltinClass {
        id: THREAD_GROUP_CLASS,
        name: "ThreadGroup",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: WARNING_MODULE,
        name: "Warning",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: OBJECTSPACE_MODULE,
        name: "ObjectSpace",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: WEAKMAP_CLASS,
        name: "ObjectSpace::WeakMap",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    // A NAMESPACE only: `WeakRef` itself is the vendored pure-Ruby
    // the `weakref` gem (CRuby's own file, `class WeakRef < Delegator`), which
    // reopens this row and declares its real superclass. The row exists
    // because `WeakRef::RefError` nests under it and because `BUILTINS` must
    // stay contiguous from `ClassId(1)`.
    BuiltinClass {
        id: WEAKREF_CLASS,
        name: "WeakRef",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        // Ungated: CRuby 4.0 has `Random::Formatter` in core, holding `#rand`
        // and `#random_number`, and `require "random/formatter"` only REOPENS
        // it to add the `hex`/`uuid`/`base64` family. zeo carries the whole
        // module either way -- see docs/COMPATIBILITY.md.
        id: RANDOM_FORMATTER_MODULE,
        name: "Random::Formatter",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: ETC_MODULE,
        name: "Etc",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("etc"),
    },
    BuiltinClass {
        id: ETC_PASSWD_CLASS,
        name: "Etc::Passwd",
        is_module: false,
        superclass: Some(STRUCT_CLASS),
        includes: &[],
        feature: Some("etc"),
    },
    BuiltinClass {
        id: ETC_GROUP_CLASS,
        name: "Etc::Group",
        is_module: false,
        superclass: Some(STRUCT_CLASS),
        includes: &[],
        feature: Some("etc"),
    },
    BuiltinClass {
        id: PATHNAME_CLASS,
        name: "Pathname",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        // CRuby's Pathname does NOT include Comparable, however much its
        // `#<=>` suggests otherwise: `Pathname.new("a") < "b"` is a
        // NoMethodError there.
        includes: &[],
        feature: None,
    },
    // The `socket` gem's hierarchy (all `require "socket"`-gated). Appended
    // after the last non-socket id so the table stays contiguous; the
    // forward edges from the earlier `Socket`(60)/`TCPSocket`(78) rows into
    // these are resolved after every class exists.
    BuiltinClass {
        id: BASIC_SOCKET_CLASS,
        name: "BasicSocket",
        is_module: false,
        superclass: Some(IO_CLASS),
        includes: &[],
        feature: Some("socket"),
    },
    BuiltinClass {
        id: IP_SOCKET_CLASS,
        name: "IPSocket",
        is_module: false,
        superclass: Some(BASIC_SOCKET_CLASS),
        includes: &[],
        feature: Some("socket"),
    },
    BuiltinClass {
        id: UDP_SOCKET_CLASS,
        name: "UDPSocket",
        is_module: false,
        superclass: Some(IP_SOCKET_CLASS),
        includes: &[],
        feature: Some("socket"),
    },
    BuiltinClass {
        id: UNIX_SOCKET_CLASS,
        name: "UNIXSocket",
        is_module: false,
        superclass: Some(BASIC_SOCKET_CLASS),
        includes: &[],
        feature: Some("socket"),
    },
    BuiltinClass {
        id: UNIX_SERVER_CLASS,
        name: "UNIXServer",
        is_module: false,
        superclass: Some(UNIX_SOCKET_CLASS),
        includes: &[],
        feature: Some("socket"),
    },
    BuiltinClass {
        id: ADDRINFO_CLASS,
        name: "Addrinfo",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("socket"),
    },
    // Ungated: `caller_locations` is core Kernel, available with no require.
    BuiltinClass {
        id: BACKTRACE_LOCATION_CLASS,
        name: "Thread::Backtrace::Location",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: FCNTL_MODULE,
        name: "Fcntl",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("fcntl"),
    },
    // Ungated: `io/console`'s methods are unconditional rows on the IO table
    // (see `docs/EXTENSIONS.md`), so the mode object they hand back has to
    // resolve without a require too.
    BuiltinClass {
        id: CONSOLE_MODE_CLASS,
        name: "IO::ConsoleMode",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: ZLIB_ZSTREAM_CLASS,
        name: "Zlib::ZStream",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("zlib"),
    },
    BuiltinClass {
        id: ZLIB_DEFLATE_CLASS,
        name: "Zlib::Deflate",
        is_module: false,
        superclass: Some(ZLIB_ZSTREAM_CLASS),
        includes: &[],
        feature: Some("zlib"),
    },
    BuiltinClass {
        id: ZLIB_INFLATE_CLASS,
        name: "Zlib::Inflate",
        is_module: false,
        superclass: Some(ZLIB_ZSTREAM_CLASS),
        includes: &[],
        feature: Some("zlib"),
    },
    BuiltinClass {
        id: ZLIB_GZIP_FILE_CLASS,
        name: "Zlib::GzipFile",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("zlib"),
    },
    BuiltinClass {
        id: ZLIB_GZIP_WRITER_CLASS,
        name: "Zlib::GzipWriter",
        is_module: false,
        superclass: Some(ZLIB_GZIP_FILE_CLASS),
        includes: &[],
        feature: Some("zlib"),
    },
    BuiltinClass {
        id: ZLIB_GZIP_READER_CLASS,
        name: "Zlib::GzipReader",
        is_module: false,
        superclass: Some(ZLIB_GZIP_FILE_CLASS),
        includes: &[ENUMERABLE_CLASS],
        feature: Some("zlib"),
    },
    BuiltinClass {
        id: PTY_MODULE,
        name: "PTY",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("pty"),
    },
    BuiltinClass {
        id: SYSLOG_MODULE,
        name: "Syslog",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("syslog"),
    },
    BuiltinClass {
        id: DIGEST_SHA384_CLASS,
        name: "Digest::SHA384",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("digest"),
    },
    BuiltinClass {
        id: DIGEST_SHA2_CLASS,
        name: "Digest::SHA2",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("digest"),
    },
    BuiltinClass {
        id: NKF_MODULE,
        name: "NKF",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("nkf"),
    },
    BuiltinClass {
        id: BIGDECIMAL_CLASS,
        name: "BigDecimal",
        is_module: false,
        superclass: Some(NUMERIC_CLASS),
        includes: &[],
        feature: Some("bigdecimal"),
    },
    // The ffi gem's runtime tier (dlopen + libffi calls), added for fiddle's
    // pure-Ruby FFI backend. `Builtin < Type` nests under it, so the `Type`
    // row precedes it. `Function < Pointer` and the constant-only
    // `AbstractMemory`/`AutoPointer` complete the gem's `is_a?` lattice.
    BuiltinClass {
        id: FFI_TYPE_CLASS,
        name: "FFI::Type",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("ffi"),
    },
    BuiltinClass {
        id: FFI_TYPE_BUILTIN_CLASS,
        name: "FFI::Type::Builtin",
        is_module: false,
        superclass: Some(FFI_TYPE_CLASS),
        includes: &[],
        feature: Some("ffi"),
    },
    BuiltinClass {
        id: FFI_DYNAMIC_LIBRARY_CLASS,
        name: "FFI::DynamicLibrary",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("ffi"),
    },
    BuiltinClass {
        id: FFI_FUNCTION_CLASS,
        name: "FFI::Function",
        is_module: false,
        superclass: Some(FFI_POINTER_CLASS),
        includes: &[],
        feature: Some("ffi"),
    },
    BuiltinClass {
        id: FFI_VARIADIC_INVOKER_CLASS,
        name: "FFI::VariadicInvoker",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("ffi"),
    },
    BuiltinClass {
        id: FFI_ABSTRACT_MEMORY_CLASS,
        name: "FFI::AbstractMemory",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("ffi"),
    },
    BuiltinClass {
        id: FFI_AUTO_POINTER_CLASS,
        name: "FFI::AutoPointer",
        is_module: false,
        superclass: Some(FFI_POINTER_CLASS),
        includes: &[],
        feature: Some("ffi"),
    },
    BuiltinClass {
        id: COVERAGE_MODULE,
        name: "Coverage",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("coverage"),
    },
    BuiltinClass {
        id: TRACEPOINT_CLASS,
        name: "TracePoint",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: OPENSSL_DIGEST_CLASS,
        name: "OpenSSL::Digest",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_DIGEST_MD4_CLASS,
        name: "OpenSSL::Digest::MD4",
        is_module: false,
        superclass: Some(OPENSSL_DIGEST_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_DIGEST_MD5_CLASS,
        name: "OpenSSL::Digest::MD5",
        is_module: false,
        superclass: Some(OPENSSL_DIGEST_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_DIGEST_RIPEMD160_CLASS,
        name: "OpenSSL::Digest::RIPEMD160",
        is_module: false,
        superclass: Some(OPENSSL_DIGEST_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_DIGEST_SHA1_CLASS,
        name: "OpenSSL::Digest::SHA1",
        is_module: false,
        superclass: Some(OPENSSL_DIGEST_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_DIGEST_SHA224_CLASS,
        name: "OpenSSL::Digest::SHA224",
        is_module: false,
        superclass: Some(OPENSSL_DIGEST_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_DIGEST_SHA256_CLASS,
        name: "OpenSSL::Digest::SHA256",
        is_module: false,
        superclass: Some(OPENSSL_DIGEST_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_DIGEST_SHA384_CLASS,
        name: "OpenSSL::Digest::SHA384",
        is_module: false,
        superclass: Some(OPENSSL_DIGEST_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_DIGEST_SHA512_CLASS,
        name: "OpenSSL::Digest::SHA512",
        is_module: false,
        superclass: Some(OPENSSL_DIGEST_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_HMAC_CLASS,
        name: "OpenSSL::HMAC",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_KDF_MODULE,
        name: "OpenSSL::KDF",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_BN_CLASS,
        name: "OpenSSL::BN",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_CIPHER_CLASS,
        name: "OpenSSL::Cipher",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_SSL_MODULE,
        name: "OpenSSL::SSL",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_SSL_CONTEXT_CLASS,
        name: "OpenSSL::SSL::SSLContext",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_SSL_SOCKET_CLASS,
        name: "OpenSSL::SSL::SSLSocket",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        // Upstream's ssl.rb writes `include Buffering` then `include
        // SocketForwarder`, which puts SocketForwarder FIRST in `ancestors`.
        includes: &[OPENSSL_BUFFERING_MODULE, OPENSSL_SOCKET_FORWARDER_MODULE],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_X509_MODULE,
        name: "OpenSSL::X509",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_X509_STORE_CLASS,
        name: "OpenSSL::X509::Store",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_X509_CERT_CLASS,
        name: "OpenSSL::X509::Certificate",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_X509_NAME_CLASS,
        name: "OpenSSL::X509::Name",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_RANDOM_MODULE,
        name: "OpenSSL::Random",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: BINDING_CLASS,
        name: "Binding",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: SOCKET_OPTION_CLASS,
        name: "Socket::Option",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: Some("socket"),
    },
    BuiltinClass {
        id: OPENSSL_BUFFERING_MODULE,
        name: "OpenSSL::Buffering",
        is_module: true,
        superclass: None,
        includes: &[ENUMERABLE_CLASS],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: OPENSSL_SOCKET_FORWARDER_MODULE,
        name: "OpenSSL::SSL::SocketForwarder",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("openssl"),
    },
    BuiltinClass {
        id: REFINEMENT_CLASS,
        name: "Refinement",
        is_module: false,
        superclass: Some(MODULE_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: PRISM_MODULE,
        name: "Prism",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("prism"),
    },
    BuiltinClass {
        id: FILE_CONSTANTS_MODULE,
        name: "File::Constants",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: ENUMERATOR_ARITHMETIC_SEQUENCE_CLASS,
        name: "Enumerator::ArithmeticSequence",
        is_module: false,
        superclass: Some(ENUMERATOR_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: PROCESS_SYS_MODULE,
        name: "Process::Sys",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: PROCESS_UID_MODULE,
        name: "Process::UID",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: PROCESS_GID_MODULE,
        name: "Process::GID",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: BACKTRACE_CLASS,
        name: "Thread::Backtrace",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: GC_PROFILER_MODULE,
        name: "GC::Profiler",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: WEAK_KEY_MAP_CLASS,
        name: "ObjectSpace::WeakKeyMap",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: RANDOM_BASE_CLASS,
        name: "Random::Base",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[RANDOM_FORMATTER_MODULE],
        feature: None,
    },
    BuiltinClass {
        id: ENUMERATOR_GENERATOR_CLASS,
        name: "Enumerator::Generator",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[ENUMERABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: ENUMERATOR_PRODUCER_CLASS,
        name: "Enumerator::Producer",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: ENCODING_CONVERTER_CLASS,
        name: "Encoding::Converter",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: UNICODE_NORMALIZE_MODULE,
        name: "UnicodeNormalize",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: SET_CORE_SET_CLASS,
        name: "Set::CoreSet",
        is_module: false,
        superclass: Some(SET_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: PROCESS_WAITER_CLASS,
        name: "Process::Waiter",
        is_module: false,
        superclass: Some(THREAD_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: RACTOR_PORT_CLASS,
        name: "Ractor::Port",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: RACTOR_MOVED_OBJECT_CLASS,
        name: "Ractor::MovedObject",
        is_module: false,
        superclass: Some(BASIC_OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: IO_BUFFER_CLASS,
        name: "IO::Buffer",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[COMPARABLE_CLASS],
        feature: None,
    },
    BuiltinClass {
        id: RUBYVM_CLASS,
        name: "RubyVM",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: RUBYVM_AST_MODULE,
        name: "RubyVM::AbstractSyntaxTree",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: RUBYVM_AST_NODE_CLASS,
        name: "RubyVM::AbstractSyntaxTree::Node",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: RUBYVM_AST_LOCATION_CLASS,
        name: "RubyVM::AbstractSyntaxTree::Location",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: RUBYVM_ISEQ_CLASS,
        name: "RubyVM::InstructionSequence",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: RUBYVM_YJIT_MODULE,
        name: "RubyVM::YJIT",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: RUBY_MODULE,
        name: "Ruby",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: RUBY_BOX_CLASS,
        name: "Ruby::Box",
        is_module: false,
        superclass: Some(MODULE_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: RUBY_BOX_ENTRY_CLASS,
        name: "Ruby::Box::Entry",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[],
        feature: None,
    },
    BuiltinClass {
        id: RUBY_BOX_LOADER_MODULE,
        name: "Ruby::Box::Loader",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: None,
    },
    // A row's POSITION here is its id (`BUILTINS` is asserted contiguous from
    // 1), so a new class is appended -- never inserted beside its relatives.
    BuiltinClass {
        id: FFI_UNION_CLASS,
        name: "FFI::Union",
        is_module: false,
        superclass: Some(FFI_STRUCT_CLASS),
        includes: &[],
        feature: Some("ffi"),
    },
    BuiltinClass {
        id: SOCKET_CONSTANTS_MODULE,
        name: "Socket::Constants",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("socket"),
    },
    BuiltinClass {
        id: CGI_ESCAPE_MODULE,
        name: "CGI::Escape",
        is_module: true,
        superclass: None,
        // It PREPENDS `CGI::EscapeExt` -- see `BUILTIN_PREPENDS`.
        includes: &[],
        feature: Some("cgi/escape"),
    },
    BuiltinClass {
        id: CGI_ESCAPE_EXT_MODULE,
        name: "CGI::EscapeExt",
        is_module: true,
        superclass: None,
        includes: &[],
        feature: Some("cgi/escape"),
    },
];
