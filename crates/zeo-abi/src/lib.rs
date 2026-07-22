//! The compiler/runtime ABI, single-sourced (Phase 15.1; hierarchy made
//! declarative in Phase 17.1).
//!
//! `zeo` (the compiler) and `zeo-rt` (the runtime every generated
//! program links) deliberately never link each other -- but they must agree
//! on the numeric identity of every built-in class: the compiler bakes
//! `ClassId`s into generated code as literals, and the runtime's dispatch/
//! `is_a?`/registry machinery interprets them. Before this crate existed,
//! that agreement was TWO parallel hand-maintained const lists
//! (`zeo::compiler` and `zeo_rt::dispatch`) synced by a
//! `debug_assert` -- a growing burden as the ABI gains class names,
//! module-ness, and (Phase 18) per-box method-table keys. This crate is the
//! one source of truth both sides re-export.
//!
//! Since Phase 17.1 the table also carries each builtin's SUPERCLASS and
//! INCLUDES -- the CRuby-exact hierarchy (oracle-verified against ruby
//! 4.0.5) that both the compiler's ancestor linearization and the runtime's
//! registry-free fallback chains are derived from. Ids are APPEND-ONLY:
//! renumbering is technically safe (nothing persists across builds), but
//! appending keeps generated-code diffs reviewable and eliminates any
//! stale-incremental-artifact risk.
//!
//! Zero dependencies, on purpose: the earlier decision against a shared
//! crate (Phase 14.4 rev.2) was about dragging the runtime's heavy deps
//! (`may`/`corosensei`) into every compiler build -- a dependency-free leaf
//! has no such cost.

/// Identifies a Ruby class at runtime AND at compile time -- the compiler
/// mirrors of this id are baked into generated code as literals, so the two
/// sides genuinely share one numbering (this type), not two synced copies.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ClassId(pub u32);

/// The first id handed out to a class created at RUNTIME (`Class.new`, and any
/// future eval-defined class -- see the runtime overlay in
/// `zeo-rt/src/runtime_meta.rs`). Compile-time ids are dense and small:
/// builtins occupy `0..~60` and user classes count up from there by program
/// size, so a `1 << 30` base leaves the entire low range to the AOT compiler
/// while staying trivially distinguishable at runtime (`id.0 >= this` answers
/// "was this class born at runtime?", which selects the overlay's ancestor-
/// walking resolution instead of the frozen flat-table fast path).
pub const RUNTIME_CLASS_ID_BASE: u32 = 1 << 30;

/// One reserved built-in class/module -- see [`BUILTINS`].
pub struct BuiltinClass {
    pub id: ClassId,
    /// The Ruby-visible name (`"Integer"`, `"Enumerable"`, ...).
    pub name: &'static str,
    /// `true` for a built-in MODULE (`Enumerable`, `Kernel`, `Math`): no
    /// superclass, never instantiated, participates in `ancestors` via
    /// `include` only.
    pub is_module: bool,
    /// CRuby's real superclass edge (oracle-verified). `None` for modules
    /// and for `BasicObject`, the true root.
    pub superclass: Option<ClassId>,
    /// CRuby's own mixins, in source order (oracle-verified): e.g.
    /// `Numeric` includes `Comparable`, `Array` includes `Enumerable`.
    pub includes: &'static [ClassId],
    /// The `require`-able feature that must be activated before this class's
    /// constant resolves -- CRuby's ext/ model, where `require "base64"`
    /// exposes `Base64`. `None` for always-on core classes (every current
    /// row except the in-tree `ext/` modules); `Some("base64")` for a
    /// require-gated extension. Referencing a gated class without its
    /// `require` is a `NameError`, exactly as in CRuby (see
    /// `zeo::Compiler::resolve_class`'s feature gate).
    pub feature: Option<&'static str>,
}

/// Whether `name` is an in-tree `ext/` feature whose `require` activates a
/// gated builtin (`"base64"` -> `Base64`). The ABI table is the single
/// source of truth for the feature -> class mapping, so the compiler's
/// require loader and its constant resolver stay in lockstep automatically
/// as extensions are added. Distinct from always-on core no-op requires
/// (`"set"`, `"tmpdir"`), which name no gated class and are handled
/// separately by the loader.
pub fn is_ext_feature(name: &str) -> bool {
    BUILTINS.iter().any(|b| b.feature == Some(name))
}

/// `ClassId(0)`, always present: the root every class ultimately chains up
/// to (via `BasicObject` since Phase 17.1). Not part of [`BUILTINS`] --
/// both sides construct/register `Object` specially (the compiler seeds it
/// as class index 0; the runtime's `Object` unit struct carries it as
/// `CLASS_ID`). Its own place in the chain is [`OBJECT_SUPERCLASS`] +
/// [`OBJECT_INCLUDES`]: `Object.ancestors == [Object, Kernel, BasicObject]`.
pub const OBJECT_CLASS: ClassId = ClassId(0);

pub const INTEGER_CLASS: ClassId = ClassId(1);
pub const FLOAT_CLASS: ClassId = ClassId(2);
pub const STRING_CLASS: ClassId = ClassId(3);
pub const SYMBOL_CLASS: ClassId = ClassId(4);
pub const ARRAY_CLASS: ClassId = ClassId(5);
pub const HASH_CLASS: ClassId = ClassId(6);
pub const RANGE_CLASS: ClassId = ClassId(7);
pub const NIL_CLASS: ClassId = ClassId(8);
pub const TRUE_CLASS: ClassId = ClassId(9);
pub const FALSE_CLASS: ClassId = ClassId(10);
pub const PROC_CLASS: ClassId = ClassId(11);
pub const REGEXP_CLASS: ClassId = ClassId(12);
pub const MATCH_DATA_CLASS: ClassId = ClassId(13);
pub const FIBER_CLASS: ClassId = ClassId(14);
pub const THREAD_CLASS: ClassId = ClassId(15);
pub const MUTEX_CLASS: ClassId = ClassId(16);
pub const QUEUE_CLASS: ClassId = ClassId(17);
pub const RACTOR_CLASS: ClassId = ClassId(18);
/// The builtin `Enumerable` MODULE -- implemented in Rust in
/// `zeo_rt::builtins::enumerable` (the enum.c architecture); `include
/// Enumerable` linearizes this id into a class's `ancestors` exactly like
/// a user module.
pub const ENUMERABLE_CLASS: ClassId = ClassId(19);
/// `Class` and `Module` (Phase 16.1) -- the classes a first-class
/// class/module VALUE (`RubyValue::Class`) answers `.class` with:
/// `Widget.class == Class`, `Enumerable.class == Module`. Both are
/// themselves CLASSES (`Class.class == Class` in real Ruby); `Class`'s
/// superclass is `Module` (`Widget.is_a?(Module)` is true).
pub const CLASS_CLASS: ClassId = ClassId(20);
pub const MODULE_CLASS: ClassId = ClassId(21);
/// The builtin `Comparable` MODULE (Phase 16.2) -- every method drives the
/// includer's own `<=>` (the compar.c architecture).
pub const COMPARABLE_CLASS: ClassId = ClassId(22);
/// Reserved for Phase 17.2's fiber-backed Enumerator; registered with the
/// CRuby-correct ancestry now so ids stay append-only.
pub const ENUMERATOR_CLASS: ClassId = ClassId(23);
/// The true root (Phase 17.1): `BasicObject.superclass` is nil in Ruby;
/// every chain ends `..., Object, Kernel, BasicObject`.
pub const BASIC_OBJECT_CLASS: ClassId = ClassId(24);
/// The `Kernel` MODULE -- Object owns ZERO instance methods in CRuby;
/// every "universal" method (`class`, `dup`, `inspect`, `is_a?`, ...) is
/// Kernel's, mixed into Object.
pub const KERNEL_CLASS: ClassId = ClassId(25);
/// The numeric tower root: `Integer`/`Float`/`Rational`/`Complex` <
/// `Numeric`, which includes `Comparable`.
pub const NUMERIC_CLASS: ClassId = ClassId(26);
pub const RATIONAL_CLASS: ClassId = ClassId(27);
pub const COMPLEX_CLASS: ClassId = ClassId(28);
/// The `Math` MODULE (module functions `Math.sqrt` etc. + `PI`/`E`).
pub const MATH_CLASS: ClassId = ClassId(29);
/// `Struct` -- root of every runtime-minted `Struct.new(...)` class (Batch E,
/// `zeo-rt`'s `rstruct`); includes `Enumerable` (CRuby).
pub const STRUCT_CLASS: ClassId = ClassId(30);
/// `Enumerator::Yielder` (Phase 17.2) -- the `y` in
/// `Enumerator.new { |y| y << 1 }`. Registered under its FLAT
/// fully-qualified name (this table has no nesting edges); user code
/// resolving the `Enumerator::Yielder` path lexically gets a loud
/// NameError -- documented, since yielders are only ever OBTAINED, never
/// named.
pub const YIELDER_CLASS: ClassId = ClassId(31);
/// The `GC` MODULE (G0) -- zeo uses `Arc` refcounting, so
/// `GC.start`/`stat`/`enable`/`disable`/`compact` are honest no-ops (see
/// `zeo_rt::dispatch`'s GC probe); the id exists so `GC` resolves as a
/// constant and `GC.start` dispatches cleanly instead of NameError-ing.
pub const GC_CLASS: ClassId = ClassId(32);
/// `IO` (G0, minimal) -- backs the `STDOUT`/`STDERR` singletons and the
/// `$stdout`/`$stderr` globals; the print family routes through whichever
/// value those globals hold. Full file-backed IO is a later phase (plan
/// P-B).
pub const IO_CLASS: ClassId = ClassId(33);
/// `Method` (G0/P4) -- the object `Kernel#method(:name)` answers; wraps a
/// bound receiver + method name and dispatches `#call` through `send`.
pub const METHOD_CLASS: ClassId = ClassId(34);

/// The plan P-B core classes. All are `RubyValue::Object(RObj)` over a
/// zeo-rt-resident struct -- `RubyValue` stays frozen at its 23 variants
/// (a new variant only pays for itself for structural Hash-key equality, a
/// codegen fast path, or an immediate; none of these qualify).
///
/// `File < IO` is CRuby's real edge, so a `File` instance answers every IO
/// instance method through the ordinary MRO walk with no duplication.
pub const FILE_CLASS: ClassId = ClassId(35);
pub const DIR_CLASS: ClassId = ClassId(36);
/// `Time` includes `Comparable` (CRuby), so `t1 < t2`/`between?`/`clamp`
/// all fall out of the existing `comparable_send` driver once `Time#<=>`
/// exists.
pub const TIME_CLASS: ClassId = ClassId(37);
/// `Process` is a MODULE (`Process.pid`, `Process::CLOCK_MONOTONIC`).
pub const PROCESS_CLASS: ClassId = ClassId(38);
/// `File::Stat` -- what `File.stat`/`File#stat` answer; the predicates
/// (`File.file?`, `.directory?`, `.size`) read through it.
pub const FILE_STAT_CLASS: ClassId = ClassId(39);
/// `Encoding` -- what `String#encoding` answers and `Encoding::UTF_8` names;
/// wraps an `encoding::EncodingId` in the runtime.
pub const ENCODING_CLASS: ClassId = ClassId(40);
/// `Data` -- root of every runtime-minted `Data.define(...)` class (Batch E,
/// `zeo-rt`'s `rstruct`). Unlike `Struct`, `Data` is immutable and NOT
/// `Enumerable` (no `each`). The one other subclassable builtin besides
/// `Struct`.
pub const DATA_CLASS: ClassId = ClassId(41);
pub const SET_CLASS: ClassId = ClassId(42);
pub const LAZY_CLASS: ClassId = ClassId(43);
/// Ruby's `Thread::ConditionVariable` (exposed top-level as `ConditionVariable`,
/// matching how `Mutex`/`Queue` are already simplified from `Thread::*`).
pub const CONDITION_VARIABLE_CLASS: ClassId = ClassId(44);
/// `Module#instance_method`'s result -- a `Method` not yet bound to a receiver.
pub const UNBOUND_METHOD_CLASS: ClassId = ClassId(45);
/// The `base64` extension's `Base64` module (require-gated, in-tree `ext/`).
pub const BASE64_MODULE: ClassId = ClassId(46);
/// `stringio`: an in-memory `IO`-like bytes buffer.
pub const STRINGIO_CLASS: ClassId = ClassId(47);
/// `strscan`: `StringScanner`, a position-tracking lexer over a String.
pub const STRING_SCANNER_CLASS: ClassId = ClassId(48);
/// `cgi/escape`: the `CGI` module's URL/HTML escape helpers.
pub const CGI_MODULE: ClassId = ClassId(49);
/// `digest`: the `Digest` framework module and its algorithm classes below.
pub const DIGEST_MODULE: ClassId = ClassId(50);
pub const DIGEST_MD5_CLASS: ClassId = ClassId(51);
pub const DIGEST_SHA1_CLASS: ClassId = ClassId(52);
pub const DIGEST_SHA256_CLASS: ClassId = ClassId(53);
pub const DIGEST_SHA512_CLASS: ClassId = ClassId(54);
/// `json`: the `JSON` module (parser/generator). Scaffolded (see docs/EXTENSIONS.md).
pub const JSON_MODULE: ClassId = ClassId(55);
/// `date`: `Date`/`DateTime`. Scaffolded.
pub const DATE_CLASS: ClassId = ClassId(56);
pub const DATETIME_CLASS: ClassId = ClassId(57);
/// `zlib`: the `Zlib` compression module. Scaffolded.
pub const ZLIB_MODULE: ClassId = ClassId(58);
/// `psych`: the `Psych` YAML module. Scaffolded.
pub const PSYCH_MODULE: ClassId = ClassId(59);
/// `socket`: the `Socket` class. Scaffolded.
pub const SOCKET_CLASS: ClassId = ClassId(60);
/// `openssl`: the `OpenSSL` module. Scaffolded.
pub const OPENSSL_MODULE: ClassId = ClassId(61);
/// `yaml`: the `YAML` module -- an alias for `Psych` (Ruby's `yaml.rb` does
/// `YAML = Psych`), so it shares the `psych` feature and dispatch.
pub const YAML_MODULE: ClassId = ClassId(62);
/// `Random` -- a seedable PRNG. An ordinary always-on core class (not an
/// ext), appended at the end of the builtin id block.
pub const RANDOM_CLASS: ClassId = ClassId(63);

/// `SizedQueue < Queue` -- a bounded blocking queue. Shares the whole `Queue`
/// method table via the ancestor chain, adding only `max`/`max=`; its
/// instances are `RubyValue::Queue` values whose runtime payload carries the
/// bound (see `zeo_rt::queue_is_sized`).
pub const SIZED_QUEUE_CLASS: ClassId = ClassId(64);

/// `Marshal` -- the object-serialization module (`Marshal.dump`/`.load`).
/// Always-on; accessed only through its class methods.
pub const MARSHAL_MODULE: ClassId = ClassId(65);

/// `Monitor` -- the reentrant lock from `require "monitor"`. Unlike `Mutex`,
/// the owning execution may re-enter it; see `ext::monitor`.
pub const MONITOR_CLASS: ClassId = ClassId(66);

/// `Process::Status` -- the wait-status object a `system`/backtick leaves in
/// `$?`. A core class (always present, no feature gate); see
/// `builtins::process`.
pub const PROCESS_STATUS_CLASS: ClassId = ClassId(67);

/// The `FFI` module (`require "ffi"`, the real `ffi` gem, #204). Require-gated
/// like the `ext/` classes but recognized as a first-class runtime namespace so
/// its `Pointer`/`MemoryPointer`/`Struct` constants resolve. `FFI::Library` is
/// NOT a row -- it's recognized syntactically (`extend FFI::Library`), never a
/// runtime constant.
pub const FFI_MODULE: ClassId = ClassId(68);
/// `FFI::Pointer` -- a wrapped C address with typed read/write accessors. See
/// `ext::ffi`.
pub const FFI_POINTER_CLASS: ClassId = ClassId(69);
/// `FFI::MemoryPointer < FFI::Pointer` -- a pointer that OWNS a heap buffer it
/// allocated; inherits every read/write accessor from `Pointer` via the
/// ancestor chain.
pub const FFI_MEMORY_POINTER_CLASS: ClassId = ClassId(70);
/// `FFI::Struct` -- root of every `class T < FFI::Struct; layout ...; end`. A
/// native-backed subclassable builtin (like `Struct`); its `layout` is
/// recognized at compile time.
pub const FFI_STRUCT_CLASS: ClassId = ClassId(71);

/// The `Signal` MODULE (`Signal.list`/`signame`/`trap`). Core and always-on
/// (CRuby exposes it unconditionally), like `Process`/`Math`.
pub const SIGNAL_MODULE: ClassId = ClassId(72);

/// The `FileTest` MODULE -- CRuby's mixin of the pure `File` predicates
/// (`exist?`/`file?`/`directory?`/...). Its module functions ARE `File`'s
/// class methods (CRuby shares one C implementation), so the runtime reuses
/// `file::lookup_class` for it rather than a parallel table.
pub const FILE_TEST_MODULE: ClassId = ClassId(73);

/// The class of the singleton `ARGF` object. CRuby names it literally
/// `"ARGF.class"` (so `ARGF.class.to_s == "ARGF.class"`); it includes
/// `Enumerable` and is never user-instantiated.
pub const ARGF_CLASS: ClassId = ClassId(74);

/// `Enumerator::Chain` -- what `Enumerator#+` and `Enumerable#chain` answer.
/// A chain IS an Enumerator (CRuby: `Enumerator::Chain.superclass ==
/// Enumerator`), so the runtime carries it as a `RubyValue::Enumerator` over
/// an `EnumSource::Chain` and reports this id from `class_of` instead of
/// giving it a separate payload type.
pub const ENUMERATOR_CHAIN_CLASS: ClassId = ClassId(75);

/// `Enumerator::Product` -- what `Enumerator.product` answers. Carried the
/// same way as [`ENUMERATOR_CHAIN_CLASS`].
pub const ENUMERATOR_PRODUCT_CLASS: ClassId = ClassId(76);

/// `Process::Tms` -- the CPU-times struct `Process.times` answers, with
/// Float members `utime`/`stime`/`cutime`/`cstime`. Carried as a plain
/// `RubyValue::Object` over `builtins::process::RTms`.
pub const PROCESS_TMS_CLASS: ClassId = ClassId(77);

/// `TCPSocket` (`require "socket"`) -- a connected TCP stream. Backed by an
/// `RIo` over the socket fd, so it inherits IO's read/write/gets surface
/// (`TCPSocket < IO`). Feature-gated on `socket`.
pub const TCPSOCKET_CLASS: ClassId = ClassId(78);

/// `TCPServer` (`require "socket"`) -- a listening TCP socket. `TCPServer <
/// TCPSocket` (CRuby's hierarchy); adds `accept`/`addr`. Any new [`BUILTINS`]
/// row must be APPENDED after this so ids stay contiguous and the exception
/// block (`FIRST_EXCEPTION_ID`) follows the last builtin.
pub const TCPSERVER_CLASS: ClassId = ClassId(79);

/// `ThreadGroup` -- always-on core (no require), like `Thread` itself. The
/// runtime models the DEFAULT group only (every thread belongs to
/// `ThreadGroup::Default`; `enclose`/re-grouping are not implemented) --
/// enough for the stdlib idiom of checking `thread.group.enclosed?` and
/// re-adding to `ThreadGroup::Default` (timeout does both).
pub const THREAD_GROUP_CLASS: ClassId = ClassId(80);

/// The `Warning` module -- category flags (`Warning[:deprecated]` /
/// `[]=`) and the `Warning.warn` sink. Always-on core: stdlib probes it
/// at load time (ostruct's `HAS_PERFORMANCE_WARNINGS`).
pub const WARNING_MODULE: ClassId = ClassId(81);

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
        includes: &[ENUMERABLE_CLASS],
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
        superclass: Some(OBJECT_CLASS),
        includes: &[ENUMERABLE_CLASS],
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
        id: CGI_MODULE,
        name: "CGI",
        is_module: true,
        superclass: None,
        includes: &[],
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
        superclass: Some(OBJECT_CLASS),
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
        superclass: Some(OBJECT_CLASS),
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
    // The real `ffi` gem (#204), require-gated on "ffi". The `FFI` module row
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
        superclass: Some(OBJECT_CLASS),
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
    BuiltinClass {
        id: PROCESS_TMS_CLASS,
        name: "Process::Tms",
        is_module: false,
        superclass: Some(OBJECT_CLASS),
        includes: &[COMPARABLE_CLASS],
        feature: None,
    },
    // `require "socket"`. TCPSocket < IO (inherits read/write/gets); TCPServer <
    // TCPSocket (adds accept/addr). KEEP THESE LAST: FIRST_EXCEPTION_ID must
    // follow the final builtin row.
    BuiltinClass {
        id: TCPSOCKET_CLASS,
        name: "TCPSocket",
        is_module: false,
        superclass: Some(IO_CLASS),
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
];

/// Top-level constant aliases for nested builtins Ruby ALSO exposes at the
/// top level: `::Queue = Thread::Queue`, `::Mutex = Thread::Mutex`, etc. The
/// class is defined `Thread::`-nested (so `.name`/`inspect` report the
/// qualified path, matching CRuby), while these aliases let bare `Queue`/
/// `Mutex`/`SizedQueue`/`ConditionVariable` still resolve at the top level.
/// `(alias_name, target_id)`; consulted last in name resolution so a user's
/// own top-level constant of the same name still wins.
pub const TOP_LEVEL_ALIASES: &[(&str, ClassId)] = &[
    ("Queue", QUEUE_CLASS),
    ("SizedQueue", SIZED_QUEUE_CLASS),
    ("Mutex", MUTEX_CLASS),
    ("ConditionVariable", CONDITION_VARIABLE_CLASS),
];

/// `Object`'s own hierarchy slot (it isn't a [`BUILTINS`] row):
/// superclass `BasicObject`, includes `Kernel` -- oracle-verified
/// `Object.ancestors == [Object, Kernel, BasicObject]`.
pub const OBJECT_SUPERCLASS: ClassId = BASIC_OBJECT_CLASS;
pub const OBJECT_INCLUDES: &[ClassId] = &[KERNEL_CLASS];

/// The first id the built-in exception classes occupy -- immediately after the
/// last [`BUILTINS`] row. DERIVED from the builtin count (`Object` is id 0 and
/// the builtins are `1..=BUILTINS.len()`, so the first free id is `len + 1`),
/// which is exactly where the compiler's sequential id counter lands after it
/// registers `Object` + every builtin. This is the whole point: **appending a
/// builtin automatically shifts the entire exception block** -- no hand-edited
/// ids, no re-learning the layout. Every [`EXCEPTION_CLASSES`] id is expressed
/// as [`exc_id`]`(offset)` off this base, so they all move together for free.
pub const FIRST_EXCEPTION_ID: u32 = BUILTINS.len() as u32 + 1;

/// The id of the exception class at position `offset` in [`EXCEPTION_CLASSES`]
/// -- the base plus its table index. Used for both the `id` and every
/// `superclass` edge so the whole block is relocatable by construction.
pub const fn exc_id(offset: u32) -> ClassId {
    ClassId(FIRST_EXCEPTION_ID + offset)
}

/// `Exception`, the root of the whole hierarchy (offset 0).
pub const EXCEPTION_CLASS: ClassId = exc_id(0);

/// `StopIteration` -- named because the runtime special-cases it (an
/// `each`-driver's terminal signal). Derived like every other exception id, so
/// it never needs a manual bump. Keep the offset in sync with its row.
pub const STOP_ITERATION_CLASS: ClassId = exc_id(15);

/// `KeyError` -- named because it exposes the typed accessors `#key`/`#receiver`
/// over an exception's hidden detail slots. Keep the offset in sync with its row.
pub const KEY_ERROR_CLASS: ClassId = exc_id(14);

/// `NameError` -- exposes `#name`/`#receiver` and a name-aware `initialize`.
pub const NAME_ERROR_CLASS: ClassId = exc_id(16);

/// `NoMethodError` (a `NameError`) -- additionally exposes `#args`.
pub const NO_METHOD_ERROR_CLASS: ClassId = exc_id(17);

/// `UncaughtThrowError` -- exposes `#tag`/`#value` from an uncaught `throw`.
pub const UNCAUGHT_THROW_ERROR_CLASS: ClassId = exc_id(47);

/// `LocalJumpError` -- exposes `#reason`/`#exit_value`.
pub const LOCAL_JUMP_ERROR_CLASS: ClassId = exc_id(20);

/// `FrozenError` -- exposes `#receiver` (the frozen object).
pub const FROZEN_ERROR_CLASS: ClassId = exc_id(23);

/// `SystemExit` -- carries an exit status via `#status`/`#success?`.
pub const SYSTEM_EXIT_CLASS: ClassId = exc_id(48);

/// `SignalException` -- resolves a signal name/number in `initialize` and
/// exposes `#signo`/`#signm`.
pub const SIGNAL_EXCEPTION_CLASS: ClassId = exc_id(49);

/// `Interrupt` (a `SignalException`) -- fixed to `SIGINT` (signo 2).
pub const INTERRUPT_CLASS: ClassId = exc_id(50);

/// One row of the built-in exception hierarchy -- the shared source of truth
/// for the ids both sides bake in.
pub struct ExceptionClass {
    pub id: ClassId,
    /// Fully-qualified Ruby name (`"ArgumentError"`, `"Encoding::CompatibilityError"`).
    pub name: &'static str,
    /// Superclass id. `None` only for the `Errno` MODULE (a namespace, not a
    /// class); every real exception class has one, up to `Exception`, whose
    /// superclass is `Object`.
    pub superclass: Option<ClassId>,
    /// `true` for the `Errno` namespace module.
    pub is_module: bool,
}

/// The built-in exception hierarchy, in the exact order the compiler registers
/// it (the [`BUILTIN_EXCEPTIONS_RB`](../zeo/parse) Ruby source, then the
/// pinned `Math::DomainError`). Ids are contiguous from [`FIRST_EXCEPTION_ID`]
/// (asserted below), so row `i` has id [`exc_id`]`(i)` -- both the `id` and
/// every `superclass` edge are written that way, so appending a builtin (which
/// bumps `FIRST_EXCEPTION_ID`) relocates the whole block automatically. This is
/// what lets
/// `zeo-rt`'s `register_exceptions` install these classes at ids the compiler
/// independently assigns the same way -- `zeo` asserts the agreement at
/// analyze time.
///
/// Superclass edges may point earlier in the table only (the source defines a
/// parent before its children); [`declared_ancestors`] linearizes them by
/// walking up to `Object`.
pub const EXCEPTION_CLASSES: &[ExceptionClass] = &[
    ExceptionClass {
        id: exc_id(0),
        name: "Exception",
        superclass: Some(OBJECT_CLASS),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(1),
        name: "ScriptError",
        superclass: Some(exc_id(0)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(2),
        name: "NotImplementedError",
        superclass: Some(exc_id(1)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(3),
        name: "LoadError",
        superclass: Some(exc_id(1)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(4),
        name: "StandardError",
        superclass: Some(exc_id(0)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(5),
        name: "ArgumentError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(6),
        name: "EncodingError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(7),
        name: "Encoding::UndefinedConversionError",
        superclass: Some(exc_id(6)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(8),
        name: "Encoding::InvalidByteSequenceError",
        superclass: Some(exc_id(6)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(9),
        name: "Encoding::CompatibilityError",
        superclass: Some(exc_id(6)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(10),
        name: "Encoding::ConverterNotFoundError",
        superclass: Some(exc_id(6)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(11),
        name: "IOError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(12),
        name: "EOFError",
        superclass: Some(exc_id(11)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(13),
        name: "IndexError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(14),
        name: "KeyError",
        superclass: Some(exc_id(13)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(15),
        name: "StopIteration",
        superclass: Some(exc_id(13)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(16),
        name: "NameError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(17),
        name: "NoMethodError",
        superclass: Some(exc_id(16)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(18),
        name: "RangeError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(19),
        name: "FloatDomainError",
        superclass: Some(exc_id(18)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(20),
        name: "LocalJumpError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(21),
        name: "RegexpError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(22),
        name: "RuntimeError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(23),
        name: "FrozenError",
        superclass: Some(exc_id(22)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(24),
        name: "NoMatchingPatternError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(25),
        name: "FiberError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(26),
        name: "ThreadError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(27),
        name: "ClosedQueueError",
        superclass: Some(exc_id(15)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(28),
        name: "RactorError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(29),
        name: "TypeError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(30),
        name: "ZeroDivisionError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(31),
        name: "SystemCallError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(32),
        name: "Errno",
        superclass: None,
        is_module: true,
    },
    ExceptionClass {
        id: exc_id(33),
        name: "Errno::ENOENT",
        superclass: Some(exc_id(31)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(34),
        name: "Errno::EACCES",
        superclass: Some(exc_id(31)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(35),
        name: "Errno::EEXIST",
        superclass: Some(exc_id(31)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(36),
        name: "Errno::ENOTDIR",
        superclass: Some(exc_id(31)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(37),
        name: "Errno::EISDIR",
        superclass: Some(exc_id(31)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(38),
        name: "Errno::ENOTEMPTY",
        superclass: Some(exc_id(31)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(39),
        name: "Errno::EPIPE",
        superclass: Some(exc_id(31)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(40),
        name: "Errno::EINVAL",
        superclass: Some(exc_id(31)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(41),
        name: "Errno::EAGAIN",
        superclass: Some(exc_id(31)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(42),
        name: "Errno::EBADF",
        superclass: Some(exc_id(31)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(43),
        name: "Errno::ESPIPE",
        superclass: Some(exc_id(31)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(44),
        name: "Errno::EXDEV",
        superclass: Some(exc_id(31)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(45),
        name: "Math::DomainError",
        superclass: Some(exc_id(4)),
        is_module: false,
    },
    // `SyntaxError < ScriptError` (#97 stage 2) -- raised by the runtime eval VM
    // when a dynamically-eval'd string fails to parse. Appended AFTER
    // `Math::DomainError` so every pre-existing exception id stays put; like
    // `Math::DomainError` it is registered in the compiler's exception-tail pin
    // rather than in `BUILTIN_EXCEPTIONS_RB` (id-ordering, not a semantic
    // difference).
    ExceptionClass {
        id: exc_id(46),
        name: "SyntaxError",
        superclass: Some(exc_id(1)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(47),
        name: "UncaughtThrowError",
        superclass: Some(exc_id(5)),
        is_module: false,
    },
    // The non-`StandardError` exception tail: a bare `rescue` never catches
    // these (they descend from `Exception` directly), so a program must name
    // them explicitly. `Interrupt < SignalException` mirrors CRuby's SIGINT
    // class. Pinned here (see `analyze::pin_builtin_exceptions_tail`).
    ExceptionClass {
        id: exc_id(48),
        name: "SystemExit",
        superclass: Some(exc_id(0)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(49),
        name: "SignalException",
        superclass: Some(exc_id(0)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(50),
        name: "Interrupt",
        superclass: Some(exc_id(49)),
        is_module: false,
    },
    // The remaining core `Exception`-tree classes CRuby defines (gem- and
    // Ractor-specific ones excluded). `NoMemoryError`/`SecurityError`/
    // `SystemStackError` descend from `Exception` directly (uncaught by a bare
    // `rescue`); the rest refine an existing `StandardError` branch.
    ExceptionClass {
        id: exc_id(51),
        name: "NoMemoryError",
        superclass: Some(exc_id(0)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(52),
        name: "SecurityError",
        superclass: Some(exc_id(0)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(53),
        name: "SystemStackError",
        superclass: Some(exc_id(0)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(54),
        name: "NoMatchingPatternKeyError",
        superclass: Some(exc_id(24)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(55),
        name: "Regexp::TimeoutError",
        superclass: Some(exc_id(21)),
        is_module: false,
    },
    ExceptionClass {
        id: exc_id(56),
        name: "IO::TimeoutError",
        superclass: Some(exc_id(11)),
        is_module: false,
    },
    // `Errno::EDOM` (a Numeric domain error, e.g. `rand(1..)`), a SystemCallError.
    ExceptionClass {
        id: exc_id(57),
        name: "Errno::EDOM",
        superclass: Some(exc_id(31)),
        is_module: false,
    },
];

/// A core class's `(superclass, includes)` edges, covering `Object`, every
/// [`BUILTINS`] row, and every [`EXCEPTION_CLASSES`] row. The single source both
/// the compiler's seeding (`Compiler::new`) and the runtime's registry
/// (`ClassRegistry::with_core`) derive the hierarchy from. Exceptions carry a
/// superclass but never `include` a module, so their `includes` is empty.
fn core_class_edges(id: ClassId) -> (Option<ClassId>, &'static [ClassId]) {
    if id == OBJECT_CLASS {
        return (Some(OBJECT_SUPERCLASS), OBJECT_INCLUDES);
    }
    if let Some(b) = BUILTINS.iter().find(|b| b.id == id) {
        return (b.superclass, b.includes);
    }
    if let Some(e) = EXCEPTION_CLASSES.iter().find(|e| e.id == id) {
        return (e.superclass, &[]);
    }
    (None, &[])
}

fn expand_core(id: ClassId, out: &mut Vec<ClassId>) {
    if out.contains(&id) {
        return;
    }
    out.push(id);
    let (superclass, includes) = core_class_edges(id);
    // `includes` reversed, then the superclass -- the exact order (and the
    // dedup above) the compiler's `mro::expand_into` uses, so an UNMODIFIED
    // core class linearizes here identically to how the compiler linearizes it.
    for &m in includes.iter().rev() {
        expand_core(m, out);
    }
    if let Some(parent) = superclass {
        expand_core(parent, out);
    }
}

/// A core class's DECLARED linearized ancestors -- what the abi declares for it
/// before any program reopens it. Covers builtins AND exceptions with one DFS:
/// e.g. `declared_ancestors(StandardError)` walks its superclass chain up to
/// `Exception`, then `Object`'s own tail, giving
/// `[StandardError, Exception, Object, Kernel, BasicObject]`.
///
/// The runtime installs these once (`ClassRegistry::with_core`); the compiler
/// emits a per-program OVERRIDE only when a program actually changes a builtin's
/// ancestors (so `class Array; include M; end` still works, full-parity, without
/// every program re-listing the unchanged hierarchy).
pub fn declared_ancestors(id: ClassId) -> Vec<ClassId> {
    let mut out = Vec::new();
    expand_core(id, &mut out);
    out
}

/// The Ruby-visible name of any builtin id, `Object` included. `None` for
/// user-class ids. Retires the runtime's hand-maintained variant->name
/// match (NoMethodError messages, registry-less display).
pub fn builtin_name(id: ClassId) -> Option<&'static str> {
    if id == OBJECT_CLASS {
        return Some("Object");
    }
    BUILTINS
        .get((id.0 as usize).wrapping_sub(1))
        .filter(|b| b.id == id)
        .map(|b| b.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The contiguity both consumers rely on: the compiler pushes
    /// `BUILTINS` in order into its class arena (so each entry's index must
    /// equal its id), and the runtime treats these ids as stable literals
    /// baked into generated programs.
    #[test]
    fn builtin_ids_are_contiguous_from_one() {
        for (i, b) in BUILTINS.iter().enumerate() {
            assert_eq!(b.id.0 as usize, i + 1, "{} out of order", b.name);
        }
    }

    /// `declared_ancestors` must match CRuby's own linearization (and thus the
    /// compiler's `mro::expand_into`, which reads the same edges), or the runtime
    /// registry and codegen would disagree on `is_a?`. Covers a builtin, a
    /// module, `Object`, and -- via the same DFS -- an exception chain.
    #[test]
    fn declared_ancestors_match_cruby() {
        let by_name = |n: &str| BUILTINS.iter().find(|b| b.name == n).unwrap().id;
        let exc = |n: &str| EXCEPTION_CLASSES.iter().find(|e| e.name == n).unwrap().id;
        let names = |ids: Vec<ClassId>| -> Vec<&'static str> {
            ids.into_iter()
                .map(|id| {
                    builtin_name(id).unwrap_or_else(|| {
                        EXCEPTION_CLASSES.iter().find(|e| e.id == id).unwrap().name
                    })
                })
                .collect()
        };
        assert_eq!(
            names(declared_ancestors(by_name("Integer"))),
            [
                "Integer",
                "Numeric",
                "Comparable",
                "Object",
                "Kernel",
                "BasicObject"
            ]
        );
        assert_eq!(
            names(declared_ancestors(by_name("Array"))),
            ["Array", "Enumerable", "Object", "Kernel", "BasicObject"]
        );
        assert_eq!(
            names(declared_ancestors(OBJECT_CLASS)),
            ["Object", "Kernel", "BasicObject"]
        );
        assert_eq!(
            names(declared_ancestors(by_name("Comparable"))),
            ["Comparable"]
        );
        // Exceptions linearize through the SAME DFS: superclass chain up to
        // `Exception`, then `Object`'s own tail. Oracle:
        // `StandardError.ancestors == [StandardError, Exception, Object, Kernel, BasicObject]`.
        assert_eq!(
            names(declared_ancestors(exc("StandardError"))),
            [
                "StandardError",
                "Exception",
                "Object",
                "Kernel",
                "BasicObject"
            ]
        );
        assert_eq!(
            names(declared_ancestors(exc("NoMethodError"))),
            [
                "NoMethodError",
                "NameError",
                "StandardError",
                "Exception",
                "Object",
                "Kernel",
                "BasicObject"
            ]
        );
        // The `Errno` namespace module: ancestors are just itself.
        assert_eq!(names(declared_ancestors(exc("Errno"))), ["Errno"]);
    }

    /// The exception classes start right after the last builtin and are
    /// contiguous, so `register_exceptions` (runtime) and the compiler's own
    /// sequential assignment land on the same id for each name.
    #[test]
    fn exception_ids_are_contiguous_after_the_builtins() {
        assert_eq!(
            FIRST_EXCEPTION_ID as usize,
            BUILTINS.len() + 1,
            "the exceptions must start right after the last builtin"
        );
        for (i, c) in EXCEPTION_CLASSES.iter().enumerate() {
            assert_eq!(
                c.id.0,
                FIRST_EXCEPTION_ID + i as u32,
                "{} out of order",
                c.name
            );
        }
    }

    /// Every hierarchy edge points at a real table entry (or Object), and
    /// modules never claim a superclass -- the shape `Compiler::new`'s
    /// table walk and the runtime's registry-free chains both assume.
    #[test]
    fn hierarchy_edges_are_well_formed() {
        let exists = |id: ClassId| id == OBJECT_CLASS || builtin_name(id).is_some();
        for b in BUILTINS {
            if b.is_module {
                assert!(b.superclass.is_none(), "module {} has a superclass", b.name);
            }
            if let Some(sup) = b.superclass {
                assert!(exists(sup), "{}'s superclass id is unknown", b.name);
                assert_ne!(sup, b.id, "{} is its own superclass", b.name);
            }
            for &inc in b.includes {
                assert!(exists(inc), "{}'s include id is unknown", b.name);
            }
        }
        assert!(exists(OBJECT_SUPERCLASS));
        for &inc in OBJECT_INCLUDES {
            assert!(exists(inc));
        }
    }

    #[test]
    fn is_ext_feature_recognizes_only_gated_builtins() {
        assert!(is_ext_feature("base64"));
        assert!(is_ext_feature("stringio"));
        assert!(is_ext_feature("strscan"));
        assert!(is_ext_feature("digest"));
        assert!(is_ext_feature("json"));
        // Always-on core classes name no feature; core no-op requires
        // (`set`/`tmpdir`) aren't gated builtins either.
        assert!(!is_ext_feature("set"));
        assert!(!is_ext_feature("tmpdir"));
        assert!(!is_ext_feature("Integer"));
        assert!(!is_ext_feature("nonesuch"));
    }

    #[test]
    fn builtin_name_answers_object_builtins_and_unknowns() {
        assert_eq!(builtin_name(OBJECT_CLASS), Some("Object"));
        assert_eq!(builtin_name(INTEGER_CLASS), Some("Integer"));
        assert_eq!(builtin_name(STRUCT_CLASS), Some("Struct"));
        assert_eq!(builtin_name(ClassId(999)), None);
    }
}
