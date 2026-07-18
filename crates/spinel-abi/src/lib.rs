//! The compiler/runtime ABI, single-sourced (Phase 15.1; hierarchy made
//! declarative in Phase 17.1).
//!
//! `spinelc` (the compiler) and `spinel-rt` (the runtime every generated
//! program links) deliberately never link each other -- but they must agree
//! on the numeric identity of every built-in class: the compiler bakes
//! `ClassId`s into generated code as literals, and the runtime's dispatch/
//! `is_a?`/registry machinery interprets them. Before this crate existed,
//! that agreement was TWO parallel hand-maintained const lists
//! (`spinelc::compiler` and `spinel_rt::dispatch`) synced by a
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
    /// `spinelc::Compiler::resolve_class`'s feature gate).
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
/// `spinel_rt::builtins::enumerable` (the enum.c architecture); `include
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
/// `Struct` -- superclass of every compile-time-synthesized
/// `Name = Struct.new(...)` class; includes `Enumerable` (CRuby).
pub const STRUCT_CLASS: ClassId = ClassId(30);
/// `Enumerator::Yielder` (Phase 17.2) -- the `y` in
/// `Enumerator.new { |y| y << 1 }`. Registered under its FLAT
/// fully-qualified name (this table has no nesting edges); user code
/// resolving the `Enumerator::Yielder` path lexically gets a loud
/// NameError -- documented, since yielders are only ever OBTAINED, never
/// named.
pub const YIELDER_CLASS: ClassId = ClassId(31);
/// The `GC` MODULE (G0) -- spinel-rs uses `Arc` refcounting, so
/// `GC.start`/`stat`/`enable`/`disable`/`compact` are honest no-ops (see
/// `spinel_rt::dispatch`'s GC probe); the id exists so `GC` resolves as a
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
/// spinel-rt-resident struct -- `RubyValue` stays frozen at its 23 variants
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
/// `Data` -- superclass of every compile-time-synthesized
/// `Name = Data.define(...)` class. Unlike `Struct`, `Data` is
/// immutable and NOT `Enumerable` (no `each`). The one other subclassable
/// builtin besides `Struct`.
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

/// Every reserved built-in class/module except `Object` (see
/// [`OBJECT_CLASS`]), in id order -- ids are contiguous from 1 by
/// construction (asserted by the unit test below), which is what lets the
/// compiler seed its class arena by pushing these in order. Superclass
/// edges may point FORWARD in the table (`Integer(1)` -> `Numeric(26)`);
/// consumers store the edge and linearize later.
pub const BUILTINS: &[BuiltinClass] = &[
    BuiltinClass { id: INTEGER_CLASS, name: "Integer", is_module: false, superclass: Some(NUMERIC_CLASS), includes: &[], feature: None },
    BuiltinClass { id: FLOAT_CLASS, name: "Float", is_module: false, superclass: Some(NUMERIC_CLASS), includes: &[], feature: None },
    BuiltinClass { id: STRING_CLASS, name: "String", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[COMPARABLE_CLASS], feature: None },
    BuiltinClass { id: SYMBOL_CLASS, name: "Symbol", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[COMPARABLE_CLASS], feature: None },
    BuiltinClass { id: ARRAY_CLASS, name: "Array", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[ENUMERABLE_CLASS], feature: None },
    BuiltinClass { id: HASH_CLASS, name: "Hash", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[ENUMERABLE_CLASS], feature: None },
    BuiltinClass { id: RANGE_CLASS, name: "Range", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[ENUMERABLE_CLASS], feature: None },
    BuiltinClass { id: NIL_CLASS, name: "NilClass", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[], feature: None },
    BuiltinClass { id: TRUE_CLASS, name: "TrueClass", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[], feature: None },
    BuiltinClass { id: FALSE_CLASS, name: "FalseClass", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[], feature: None },
    BuiltinClass { id: PROC_CLASS, name: "Proc", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[], feature: None },
    BuiltinClass { id: REGEXP_CLASS, name: "Regexp", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[], feature: None },
    BuiltinClass { id: MATCH_DATA_CLASS, name: "MatchData", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[], feature: None },
    BuiltinClass { id: FIBER_CLASS, name: "Fiber", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[], feature: None },
    BuiltinClass { id: THREAD_CLASS, name: "Thread", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[], feature: None },
    BuiltinClass { id: MUTEX_CLASS, name: "Mutex", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[], feature: None },
    BuiltinClass { id: QUEUE_CLASS, name: "Queue", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[], feature: None },
    BuiltinClass { id: RACTOR_CLASS, name: "Ractor", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[], feature: None },
    BuiltinClass { id: ENUMERABLE_CLASS, name: "Enumerable", is_module: true, superclass: None, includes: &[], feature: None },
    BuiltinClass { id: CLASS_CLASS, name: "Class", is_module: false, superclass: Some(MODULE_CLASS), includes: &[], feature: None },
    BuiltinClass { id: MODULE_CLASS, name: "Module", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[], feature: None },
    BuiltinClass { id: COMPARABLE_CLASS, name: "Comparable", is_module: true, superclass: None, includes: &[], feature: None },
    BuiltinClass { id: ENUMERATOR_CLASS, name: "Enumerator", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[ENUMERABLE_CLASS], feature: None },
    BuiltinClass { id: BASIC_OBJECT_CLASS, name: "BasicObject", is_module: false, superclass: None, includes: &[], feature: None },
    BuiltinClass { id: KERNEL_CLASS, name: "Kernel", is_module: true, superclass: None, includes: &[], feature: None },
    BuiltinClass { id: NUMERIC_CLASS, name: "Numeric", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[COMPARABLE_CLASS], feature: None },
    BuiltinClass { id: RATIONAL_CLASS, name: "Rational", is_module: false, superclass: Some(NUMERIC_CLASS), includes: &[], feature: None },
    BuiltinClass { id: COMPLEX_CLASS, name: "Complex", is_module: false, superclass: Some(NUMERIC_CLASS), includes: &[], feature: None },
    BuiltinClass { id: MATH_CLASS, name: "Math", is_module: true, superclass: None, includes: &[], feature: None },
    BuiltinClass { id: STRUCT_CLASS, name: "Struct", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[ENUMERABLE_CLASS], feature: None },
    BuiltinClass { id: YIELDER_CLASS, name: "Enumerator::Yielder", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[], feature: None },
    BuiltinClass { id: GC_CLASS, name: "GC", is_module: true, superclass: None, includes: &[], feature: None },
    BuiltinClass { id: IO_CLASS, name: "IO", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[ENUMERABLE_CLASS], feature: None },
    BuiltinClass { id: METHOD_CLASS, name: "Method", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[], feature: None },
    BuiltinClass { id: FILE_CLASS, name: "File", is_module: false, superclass: Some(IO_CLASS), includes: &[], feature: None },
    BuiltinClass { id: DIR_CLASS, name: "Dir", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[ENUMERABLE_CLASS], feature: None },
    BuiltinClass { id: TIME_CLASS, name: "Time", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[COMPARABLE_CLASS], feature: None },
    BuiltinClass { id: PROCESS_CLASS, name: "Process", is_module: true, superclass: None, includes: &[], feature: None },
    BuiltinClass { id: FILE_STAT_CLASS, name: "File::Stat", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[COMPARABLE_CLASS], feature: None },
    BuiltinClass { id: ENCODING_CLASS, name: "Encoding", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[], feature: None },
    BuiltinClass { id: DATA_CLASS, name: "Data", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[], feature: None },
    BuiltinClass { id: SET_CLASS, name: "Set", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[ENUMERABLE_CLASS], feature: None },
    BuiltinClass { id: LAZY_CLASS, name: "Enumerator::Lazy", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[ENUMERABLE_CLASS], feature: None },
    BuiltinClass { id: CONDITION_VARIABLE_CLASS, name: "ConditionVariable", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[], feature: None },
    BuiltinClass { id: UNBOUND_METHOD_CLASS, name: "UnboundMethod", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[], feature: None },
    BuiltinClass { id: BASE64_MODULE, name: "Base64", is_module: true, superclass: None, includes: &[], feature: Some("base64") },
    // In-tree `ext/` extensions -- CRuby's ext/ model. Each is require-gated
    // (its constant is invisible until its `require` fires) AND compile-gated
    // by a per-extension cargo feature on `spinel-rt` (see that crate's
    // `[features]` and `ext/mod.rs`). Some carry real implementations, others
    // are scaffolded (a couple methods, the rest `todo!`) -- see
    // `docs/EXTENSIONS.md` for the per-extension status.
    BuiltinClass { id: STRINGIO_CLASS, name: "StringIO", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[ENUMERABLE_CLASS], feature: Some("stringio") },
    BuiltinClass { id: STRING_SCANNER_CLASS, name: "StringScanner", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[], feature: Some("strscan") },
    BuiltinClass { id: CGI_MODULE, name: "CGI", is_module: true, superclass: None, includes: &[], feature: Some("cgi/escape") },
    BuiltinClass { id: DIGEST_MODULE, name: "Digest", is_module: true, superclass: None, includes: &[], feature: Some("digest") },
    BuiltinClass { id: DIGEST_MD5_CLASS, name: "Digest::MD5", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[], feature: Some("digest") },
    BuiltinClass { id: DIGEST_SHA1_CLASS, name: "Digest::SHA1", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[], feature: Some("digest") },
    BuiltinClass { id: DIGEST_SHA256_CLASS, name: "Digest::SHA256", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[], feature: Some("digest") },
    BuiltinClass { id: DIGEST_SHA512_CLASS, name: "Digest::SHA512", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[], feature: Some("digest") },
    BuiltinClass { id: JSON_MODULE, name: "JSON", is_module: true, superclass: None, includes: &[], feature: Some("json") },
    BuiltinClass { id: DATE_CLASS, name: "Date", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[COMPARABLE_CLASS], feature: Some("date") },
    BuiltinClass { id: DATETIME_CLASS, name: "DateTime", is_module: false, superclass: Some(DATE_CLASS), includes: &[], feature: Some("date") },
    BuiltinClass { id: ZLIB_MODULE, name: "Zlib", is_module: true, superclass: None, includes: &[], feature: Some("zlib") },
    BuiltinClass { id: PSYCH_MODULE, name: "Psych", is_module: true, superclass: None, includes: &[], feature: Some("psych") },
    BuiltinClass { id: SOCKET_CLASS, name: "Socket", is_module: false, superclass: Some(OBJECT_CLASS), includes: &[], feature: Some("socket") },
    BuiltinClass { id: OPENSSL_MODULE, name: "OpenSSL", is_module: true, superclass: None, includes: &[], feature: Some("openssl") },
    // `YAML` is `Psych` under Ruby's `yaml.rb` (`YAML = Psych`); it shares the
    // `psych` feature so `require "yaml"` (canonicalized to `psych` by the
    // loader) makes both constants resolve, and both dispatch to `ext::psych`.
    BuiltinClass { id: YAML_MODULE, name: "YAML", is_module: true, superclass: None, includes: &[], feature: Some("psych") },
];

/// `Object`'s own hierarchy slot (it isn't a [`BUILTINS`] row):
/// superclass `BasicObject`, includes `Kernel` -- oracle-verified
/// `Object.ancestors == [Object, Kernel, BasicObject]`.
pub const OBJECT_SUPERCLASS: ClassId = BASIC_OBJECT_CLASS;
pub const OBJECT_INCLUDES: &[ClassId] = &[KERNEL_CLASS];

/// The first id the exception prelude occupies -- immediately after the last
/// [`BUILTINS`] row (`YAML_MODULE` = 62). See [`EXCEPTION_PRELUDE_CLASSES`].
pub const FIRST_PRELUDE_ID: u32 = 63;

/// `Exception`, the root of the whole hierarchy.
pub const EXCEPTION_CLASS: ClassId = ClassId(FIRST_PRELUDE_ID);

/// One row of the built-in exception hierarchy -- the shared source of truth
/// for the ids both sides bake in.
pub struct PreludeClass {
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
/// it (the [`EXCEPTION_PRELUDE`](../spinelc/parse) Ruby source, then the pinned
/// `Math::DomainError`). Ids are contiguous from [`FIRST_PRELUDE_ID`] (asserted
/// below), so index `i` has id `63 + i`. This is what lets `spinel-rt`'s
/// `register_prelude` install these classes at ids the compiler independently
/// assigns the same way -- `spinelc` asserts the agreement at analyze time.
///
/// Superclass edges may point earlier in the table only (the source defines a
/// parent before its children); `register_prelude` linearizes ancestors by
/// walking them up to `Object`.
pub const EXCEPTION_PRELUDE_CLASSES: &[PreludeClass] = &[
    PreludeClass { id: ClassId(63), name: "Exception", superclass: Some(OBJECT_CLASS), is_module: false },
    PreludeClass { id: ClassId(64), name: "ScriptError", superclass: Some(ClassId(63)), is_module: false },
    PreludeClass { id: ClassId(65), name: "NotImplementedError", superclass: Some(ClassId(64)), is_module: false },
    PreludeClass { id: ClassId(66), name: "LoadError", superclass: Some(ClassId(64)), is_module: false },
    PreludeClass { id: ClassId(67), name: "StandardError", superclass: Some(ClassId(63)), is_module: false },
    PreludeClass { id: ClassId(68), name: "ArgumentError", superclass: Some(ClassId(67)), is_module: false },
    PreludeClass { id: ClassId(69), name: "EncodingError", superclass: Some(ClassId(67)), is_module: false },
    PreludeClass { id: ClassId(70), name: "Encoding::UndefinedConversionError", superclass: Some(ClassId(69)), is_module: false },
    PreludeClass { id: ClassId(71), name: "Encoding::InvalidByteSequenceError", superclass: Some(ClassId(69)), is_module: false },
    PreludeClass { id: ClassId(72), name: "Encoding::CompatibilityError", superclass: Some(ClassId(69)), is_module: false },
    PreludeClass { id: ClassId(73), name: "Encoding::ConverterNotFoundError", superclass: Some(ClassId(69)), is_module: false },
    PreludeClass { id: ClassId(74), name: "IOError", superclass: Some(ClassId(67)), is_module: false },
    PreludeClass { id: ClassId(75), name: "EOFError", superclass: Some(ClassId(74)), is_module: false },
    PreludeClass { id: ClassId(76), name: "IndexError", superclass: Some(ClassId(67)), is_module: false },
    PreludeClass { id: ClassId(77), name: "KeyError", superclass: Some(ClassId(76)), is_module: false },
    PreludeClass { id: ClassId(78), name: "StopIteration", superclass: Some(ClassId(76)), is_module: false },
    PreludeClass { id: ClassId(79), name: "NameError", superclass: Some(ClassId(67)), is_module: false },
    PreludeClass { id: ClassId(80), name: "NoMethodError", superclass: Some(ClassId(79)), is_module: false },
    PreludeClass { id: ClassId(81), name: "RangeError", superclass: Some(ClassId(67)), is_module: false },
    PreludeClass { id: ClassId(82), name: "FloatDomainError", superclass: Some(ClassId(81)), is_module: false },
    PreludeClass { id: ClassId(83), name: "LocalJumpError", superclass: Some(ClassId(67)), is_module: false },
    PreludeClass { id: ClassId(84), name: "RegexpError", superclass: Some(ClassId(67)), is_module: false },
    PreludeClass { id: ClassId(85), name: "RuntimeError", superclass: Some(ClassId(67)), is_module: false },
    PreludeClass { id: ClassId(86), name: "FrozenError", superclass: Some(ClassId(85)), is_module: false },
    PreludeClass { id: ClassId(87), name: "NoMatchingPatternError", superclass: Some(ClassId(67)), is_module: false },
    PreludeClass { id: ClassId(88), name: "FiberError", superclass: Some(ClassId(67)), is_module: false },
    PreludeClass { id: ClassId(89), name: "ThreadError", superclass: Some(ClassId(67)), is_module: false },
    PreludeClass { id: ClassId(90), name: "ClosedQueueError", superclass: Some(ClassId(78)), is_module: false },
    PreludeClass { id: ClassId(91), name: "RactorError", superclass: Some(ClassId(67)), is_module: false },
    PreludeClass { id: ClassId(92), name: "TypeError", superclass: Some(ClassId(67)), is_module: false },
    PreludeClass { id: ClassId(93), name: "ZeroDivisionError", superclass: Some(ClassId(67)), is_module: false },
    PreludeClass { id: ClassId(94), name: "SystemCallError", superclass: Some(ClassId(67)), is_module: false },
    PreludeClass { id: ClassId(95), name: "Errno", superclass: None, is_module: true },
    PreludeClass { id: ClassId(96), name: "Errno::ENOENT", superclass: Some(ClassId(94)), is_module: false },
    PreludeClass { id: ClassId(97), name: "Errno::EACCES", superclass: Some(ClassId(94)), is_module: false },
    PreludeClass { id: ClassId(98), name: "Errno::EEXIST", superclass: Some(ClassId(94)), is_module: false },
    PreludeClass { id: ClassId(99), name: "Errno::ENOTDIR", superclass: Some(ClassId(94)), is_module: false },
    PreludeClass { id: ClassId(100), name: "Errno::EISDIR", superclass: Some(ClassId(94)), is_module: false },
    PreludeClass { id: ClassId(101), name: "Errno::ENOTEMPTY", superclass: Some(ClassId(94)), is_module: false },
    PreludeClass { id: ClassId(102), name: "Errno::EPIPE", superclass: Some(ClassId(94)), is_module: false },
    PreludeClass { id: ClassId(103), name: "Errno::EINVAL", superclass: Some(ClassId(94)), is_module: false },
    PreludeClass { id: ClassId(104), name: "Errno::EAGAIN", superclass: Some(ClassId(94)), is_module: false },
    PreludeClass { id: ClassId(105), name: "Errno::EBADF", superclass: Some(ClassId(94)), is_module: false },
    PreludeClass { id: ClassId(106), name: "Errno::ESPIPE", superclass: Some(ClassId(94)), is_module: false },
    PreludeClass { id: ClassId(107), name: "Errno::EXDEV", superclass: Some(ClassId(94)), is_module: false },
    PreludeClass { id: ClassId(108), name: "Math::DomainError", superclass: Some(ClassId(67)), is_module: false },
];

/// The tail every exception's linearized ancestors ends with, after its own
/// superclass chain reaches `Exception`: `Object`'s own ancestors. Oracle:
/// `StandardError.ancestors[-3..] == [Object, Kernel, BasicObject]`.
pub const OBJECT_ANCESTRY_TAIL: &[ClassId] = &[OBJECT_CLASS, KERNEL_CLASS, BASIC_OBJECT_CLASS];

/// A builtin's `(superclass, includes)` edges, `Object` included. The single
/// source both the compiler's seeding (`Compiler::new`) and the runtime's
/// `register_builtins` derive the hierarchy from.
fn builtin_edges(id: ClassId) -> (Option<ClassId>, &'static [ClassId]) {
    if id == OBJECT_CLASS {
        return (Some(OBJECT_SUPERCLASS), OBJECT_INCLUDES);
    }
    match BUILTINS.iter().find(|b| b.id == id) {
        Some(b) => (b.superclass, b.includes),
        None => (None, &[]),
    }
}

fn expand_builtin(id: ClassId, out: &mut Vec<ClassId>) {
    if out.contains(&id) {
        return;
    }
    out.push(id);
    let (superclass, includes) = builtin_edges(id);
    // `includes` reversed, then the superclass -- the exact order (and the
    // dedup above) the compiler's `mro::expand_into` uses, so an UNMODIFIED
    // builtin linearizes here identically to how the compiler linearizes it.
    for &m in includes.iter().rev() {
        expand_builtin(m, out);
    }
    if let Some(parent) = superclass {
        expand_builtin(parent, out);
    }
}

/// A builtin's DEFAULT linearized ancestors -- what it has before any program
/// reopens it to `include`/`prepend` a module. `register_builtins` (runtime)
/// installs these once; the compiler emits a per-program OVERRIDE only when a
/// program actually changes them (so `class Array; include M; end` still works,
/// full-parity, without every program re-listing the unchanged hierarchy).
pub fn default_builtin_ancestors(id: ClassId) -> Vec<ClassId> {
    let mut out = Vec::new();
    expand_builtin(id, &mut out);
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

    /// `default_builtin_ancestors` must match CRuby's own linearization (and
    /// thus the compiler's `mro::expand_into`, which reads the same edges), or
    /// the runtime `register_builtins` and codegen would disagree on `is_a?`.
    #[test]
    fn default_ancestors_match_cruby() {
        let by_name = |n: &str| BUILTINS.iter().find(|b| b.name == n).unwrap().id;
        let names = |ids: Vec<ClassId>| -> Vec<&'static str> {
            ids.into_iter().map(|id| builtin_name(id).unwrap()).collect()
        };
        assert_eq!(
            names(default_builtin_ancestors(by_name("Integer"))),
            ["Integer", "Numeric", "Comparable", "Object", "Kernel", "BasicObject"]
        );
        assert_eq!(
            names(default_builtin_ancestors(by_name("Array"))),
            ["Array", "Enumerable", "Object", "Kernel", "BasicObject"]
        );
        assert_eq!(
            names(default_builtin_ancestors(OBJECT_CLASS)),
            ["Object", "Kernel", "BasicObject"]
        );
        assert_eq!(
            names(default_builtin_ancestors(by_name("Comparable"))),
            ["Comparable"]
        );
    }

    /// The exception prelude starts right after the last builtin and is
    /// contiguous, so `register_prelude` (runtime) and the compiler's own
    /// sequential assignment land on the same id for each name.
    #[test]
    fn prelude_ids_are_contiguous_after_the_builtins() {
        assert_eq!(
            FIRST_PRELUDE_ID as usize,
            BUILTINS.len() + 1,
            "the prelude must start right after the last builtin"
        );
        for (i, c) in EXCEPTION_PRELUDE_CLASSES.iter().enumerate() {
            assert_eq!(
                c.id.0,
                FIRST_PRELUDE_ID + i as u32,
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
