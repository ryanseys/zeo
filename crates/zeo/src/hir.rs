//! A real typed HIR instead of a text-serialized node table. See the plan's
//! "Compiler internals" section: zeo's `zeo_parse.c` serializes
//! Prism's C AST to a line-oriented text format that `node_table.c`
//! re-parses into a flat, dynamically-typed `SpNode` arena.
//! Since `ruby-prism` hands us a real, safe, in-process `Node` tree
//! directly, we lower straight from `ruby_prism::Node` into this typed
//! arena: one step instead of two, and no string-keyed dynamic field lookup
//! anywhere.
//!
//! Identifiers (class/method/ivar/local names) are plain `String`s here, not
//! a compact interned id. Interning them is a possible later optimization,
//! not a blocker.

#![allow(
    clippy::wildcard_enum_match_arm,
    reason = "not yet swept for wildcard arms -- see the lint's note in lib.rs"
)]

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct NodeId(u32);

/// One per-node boolean fact, as a bit in [`Hir`]'s `flags` array. Read and
/// written through [`Hir::has_flag`]/[`Hir::set_flag`].
///
/// Each of these was its own `HashSet<NodeId>` side table. They are facts a
/// node's own variant cannot express -- two nodes of the same shape that mean
/// different things -- which is why they live beside the arena rather than in
/// `HirNode`, and why adding one here does not disturb the dozen walkers that
/// match on the variant.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NodeFlag(u16);

impl NodeFlag {
    /// A `Call` that is a VCALL (prism's `is_variable_call`: a bare identifier,
    /// implicit self, no args/parens -- something that could have been a
    /// local). A miss on one raises `NameError`, not `NoMethodError`; codegen
    /// routes these through `send_value_vcall_in`.
    pub const VCALL: NodeFlag = NodeFlag(1 << 0);
    /// A literal block handed to a RE-HOMING call (`recv.instance_eval { }` and
    /// the exec/class_eval family): its `self` becomes the receiver at run
    /// time, so codegen forces the self capture (`procs`) and its call sites
    /// ask the runtime `self`'s class the `protected` question instead of
    /// baking the lexical one (`visibility::caller_class`).
    pub const REHOMED_BLOCK: NodeFlag = NodeFlag(1 << 1);
    /// A `ClassVarRead` born as the READ half of `@@x ||= v`: ruby's ONE
    /// lenient cvar read -- an unassigned `@@x` reads as nil there and the
    /// write then defines it, where every other read (including `+=`/`&&=`)
    /// raises NameError. The `ConstReadOrNil` rule, applied to cvars.
    pub const LENIENT_CVAR_READ: NodeFlag = NodeFlag(1 << 2);
    /// A literal block on a COMPUTED-name `define_method(name) { }` call -- the
    /// literal-symbol form desugars to `DefMethod` and never gets here. The
    /// block body IS a method body at run time, so `super` inside it resolves
    /// through the runtime method-frame stack and a BARE `super` raises ruby's
    /// define_method refusal (see `procs::emit_proc_value`).
    pub const DYNAMIC_DEFINE_METHOD_BLOCK: NodeFlag = NodeFlag(1 << 3);
    /// A `DefMethod` desugared from a literal-symbol `define_method(:x) { }` /
    /// `define_singleton_method(:x) { }` -- a BLOCK, not a `def`. Ruby rejects
    /// a `class` keyword inside a `def` and accepts one inside a block, so the
    /// analyze walk has to tell the two apart even though the desugar gives
    /// them one node type (`collect_nested_bodies`).
    ///
    /// The flagged nodes are also enumerated, in push order, through
    /// [`Hir::block_bodied_defs`].
    pub const BLOCK_BODIED_DEF: NodeFlag = NodeFlag(1 << 4);
    /// A `HashLit` that is a `yield`'s KEYWORD arguments folded into one
    /// trailing hash, rather than a hash the source really wrote. The two are
    /// the same shape but not the same value: `yield(1, **h)` with an empty `h`
    /// passes only `1`, where `yield(1, {})` passes the hash.
    pub const KWARGS_HASH: NodeFlag = NodeFlag(1 << 5);
    /// A `DefMethod` that `attr_reader`/`attr_writer`/`attr_accessor`/`attr`
    /// SYNTHESIZED, as opposed to a `def` the source really wrote.
    ///
    /// The two are the same shape, and codegen deliberately treats them the
    /// same everywhere but one place: CRuby compiles an `attr_*` accessor to an
    /// iseq-less method, which fires no `:call`/`:return` `TracePoint` event,
    /// where a hand-written `def x; @x; end` is an ordinary method and does. So
    /// a synthesized accessor may devirtualize even under tracing -- see
    /// `codegen::emit_class`.
    pub const ATTR_GENERATED: NodeFlag = NodeFlag(1 << 6);
    /// A `DefMethod` written inside a CONSTANT-BEARING `class << self` body.
    /// Its lexical home is the singleton class -- a bare constant there
    /// resolves against the singleton's surrogate first, and `Module.nesting`
    /// reports it (`analyze::register_method` sets `Scope::lexical_home` from
    /// this). Defs in a constant-free singleton body stay untagged: with no
    /// surrogate there is nothing to resolve differently.
    pub const SINGLETON_BODY_DEF: NodeFlag = NodeFlag(1 << 7);
    /// The receiver of a call the SOURCE wrote no receiver for -- see
    /// [`Hir::note_implicit_self_receiver`].
    pub const IMPLICIT_SELF_RECEIVER: NodeFlag = NodeFlag(1 << 8);
    /// A `def` the `ruby2_keywords` directive marked. Its `*rest` keeps the
    /// KEYWORD mark on a trailing hash it captured, so forwarding it through a
    /// splat re-promotes it to keywords instead of passing it positionally.
    /// That is the whole point of the directive: a method written before ruby
    /// 3 separated them can still forward either kind unchanged.
    pub const RUBY2_KEYWORDS: NodeFlag = NodeFlag(1 << 9);
}

/// Index into `Hir::files` -- which source file a `Span` points into.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FileId(pub u32);

/// A byte range in one source file, straight from prism's own offsets.
/// Stamped per-NODE in `Hir::spans` (parallel to the arena, so `HirNode`
/// itself carries no span field) by the `lower_node` wrapper; a node pushed
/// outside any lowering frame -- a synthetic desugaring node, the `Program`
/// root -- gets `Span::SYNTH`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Span {
    pub file: FileId,
    pub start: u32,
    pub end: u32,
}

impl Span {
    /// The no-provenance sentinel: a synthetic node, or one lowered from a
    /// source string no file entry was registered for (the exception
    /// prelude, `eval` bodies). `Span::SYNTH.known()` is `None`.
    pub const SYNTH: Span = Span {
        file: FileId(u32::MAX),
        start: 0,
        end: 0,
    };

    /// `Some(self)` for a real span, `None` for `SYNTH`.
    pub fn known(self) -> Option<Span> {
        (self.file.0 != u32::MAX).then_some(self)
    }
}

/// Where a script's `__END__` DATA section starts. Ruby exposes those bytes as
/// an open `File` in `DATA`, seeked past the marker -- so what travels to the
/// runtime is a path plus an offset, not the bytes: `DATA.rewind` seeks to
/// offset 0 of the SOURCE file, which only a real handle on it can do.
///
/// The path is absolutized at compile time, since the binary can run from any
/// directory.
#[derive(Clone)]
pub struct DataSection {
    pub path: String,
    pub offset: u64,
}

/// One registered source file: the name diagnostics display (the path as
/// given, `"-e"`, ...) plus the full source text a renderer excerpts from.
#[derive(Clone)]
pub struct SourceFile {
    pub name: String,
    /// Shared rather than owned: the loader parses from this same text and
    /// must keep it alive while `ruby_prism` borrows it, so an owned `String`
    /// here forced a full copy of every Ruby file it read.
    pub source: std::sync::Arc<str>,
    /// This file's OWN `# frozen_string_literal: true` magic comment (each
    /// required file carries its own, not the entry file's).
    pub frozen_string_literal: bool,
    /// Byte offset of each line's start (`[0]` is always 0), built once at
    /// registration -- `line_at` answers in a binary search instead of
    /// re-counting newlines from byte 0, which made per-statement line
    /// stamping quadratic in file size at gem scale.
    line_starts: Vec<u32>,
}

impl SourceFile {
    /// The 1-based line containing byte offset `byte`: exactly
    /// `1 + newlines strictly before byte`, the same count the old linear
    /// scan produced (a start `s = i + 1` satisfies `s <= byte` iff the
    /// newline at `i` sits strictly before `byte`).
    pub fn line_at(&self, byte: u32) -> u32 {
        self.line_starts.partition_point(|&s| s <= byte) as u32
    }
}

/// A `# frozen_string_literal: true` magic comment in the leading comment
/// block (after an optional shebang). CRuby only honors it there, before any
/// code; a blank or code line ends the region.
pub fn magic_frozen_string_literal(source: &str) -> bool {
    for (i, line) in source.lines().enumerate() {
        let line = line.trim_start();
        if i == 0 && line.starts_with("#!") {
            continue;
        }
        if !line.starts_with('#') {
            break;
        }
        let lower = line.to_ascii_lowercase();
        if let Some(idx) = lower.find("frozen_string_literal") {
            let rest = line[idx + "frozen_string_literal".len()..].trim_start();
            if let Some(rest) = rest.strip_prefix(':') {
                let value: String = rest
                    .trim_start()
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect();
                return value.eq_ignore_ascii_case("true");
            }
        }
    }
    false
}

/// A local the COMPILER introduced -- an evaluate-once receiver/index
/// binding, an assignment's captured right-hand side, a destructuring
/// param's slot. Ruby's `local_variables` reports only names the source
/// wrote, so these must never appear there.
pub fn is_internal_local(name: &str) -> bool {
    Hir::INTERNAL_LOCAL_PREFIXES
        .iter()
        .any(|p| name.starts_with(p))
}

#[derive(Clone, Default)]
pub struct Hir {
    nodes: Vec<HirNode>,
    /// [`Hir::uses_proc_binding`]'s memo -- computed on first ask, after
    /// lowering has finished adding nodes.
    proc_binding: std::sync::OnceLock<bool>,
    /// Per-node provenance, parallel to `nodes` -- see `Span`.
    spans: Vec<Span>,
    /// Every FFI type name declared so far (`typedef`/`enum`/`callback`), so a
    /// nested body can name one its ENCLOSING library declared -- sassc writes
    /// `SassTag = enum(...)` in `module Native` and then `layout :tag, SassTag`
    /// inside a struct class nested in it, which is a different class body and
    /// a different alias map.
    ///
    /// A name redeclared to a DIFFERENT type is poisoned (`None`) rather than
    /// overwritten: two libraries may legitimately use one name for two types,
    /// and each still resolves it from its OWN map. Only this cross-body
    /// fallback becomes unavailable, so the result is a clean "isn't a declared
    /// FFI type" rejection instead of a silently wrong width.
    pub ffi_types: crate::compiler::FMap<String, Option<FfiType>>,
    /// Module paths that have been `extend FFI::Library`'d -- see
    /// [`Hir::mark_ffi_library`].
    ffi_library_crefs: crate::compiler::FSet<String>,
    /// Leaf names of every `class X < FFI::Struct` the lowering has seen --
    /// including ones whose `layout` never lowered (a DSL-built layout,
    /// ffi_dry's `dsl_layout`). A SIGNATURE position only needs the
    /// by-reference fact, so these enter the type table as [`FfiType::
    /// StructRef`] when no layout/typedef claims the name.
    ffi_struct_classes: crate::compiler::FSet<String>,
    /// Per-node boolean facts, one `u16` per node and parallel to
    /// `nodes`/`spans` -- see [`NodeFlag`] for what each bit means and
    /// [`Hir::set_flag`]/[`Hir::has_flag`] for the accessors.
    ///
    /// These were nine separate `HashSet<NodeId>`/`HashMap<NodeId, _>` side
    /// tables. `NodeId` is a dense `u32`, so every probe hashed a small integer
    /// -- with SipHash, on paths that ask per node (the class-body walk asks
    /// `BLOCK_BODIED_DEF` of every statement, codegen asks `VCALL` and
    /// `KWARGS_HASH` per call site). An array index answers the same question
    /// with no hash at all, and nine tables' worth of allocation becomes two
    /// bytes per node.
    flags: Vec<u16>,
    /// The [`NodeFlag::BLOCK_BODIED_DEF`] nodes in push order.
    ///
    /// `flags` answers "is this one?" in an array index, but codegen's
    /// inline-marker walk also has to ENUMERATE them
    /// (`codegen::inline_class_markers`), which a bit array cannot do without
    /// scanning the whole arena. Push order also makes that walk deterministic,
    /// which iterating a `HashSet` never was.
    block_bodied_def_list: Vec<NodeId>,
    /// Every constant SPELLING the program assigns, mapped to the value nodes
    /// assigned to it -- see [`Hir::const_write_values`].
    ///
    /// The key is the spelling as written: `"M::D"` for an explicit
    /// `M::D = ...`, the bare leaf otherwise. That is what the three
    /// "is this constant assigned?" predicates in `lower::defs` matched by
    /// hand, each sweeping the WHOLE arena per `class` statement and
    /// `format!`-ing the scoped spelling per visited `ConstWrite` to do it.
    ///
    /// Filled during lowering rather than in one sweep afterwards, and that is
    /// load-bearing rather than incidental: those predicates ask what is
    /// assigned SO FAR, which is the same "defined earlier in the file" rule
    /// `Compiler::resolve_class` applies. A map built after lowering would
    /// answer for later statements too and reroute definitions accordingly.
    const_writes: crate::compiler::FMap<String, Vec<NodeId>>,
    /// Statements a `class << self` body contributed to its ENCLOSING class
    /// body, mapped to the `class << self` node itself. The singleton
    /// mapping splices them in place (that is the retagging model), so
    /// without this they would run in the enclosing body's frame and every
    /// backtrace raised through one would be a frame short -- see
    /// `codegen::stmt::emit_body`, which groups consecutive entries under one
    /// `singleton class` frame.
    pub singleton_frame_stmts: crate::compiler::FMap<NodeId, NodeId>,
    /// `DefMethod` nodes an `alias` cloned, mapped to the name they were born
    /// under -- see [`record_alias_origin`](Self::record_alias_origin).
    alias_origins: crate::compiler::FMap<NodeId, String>,
    /// `ClassDef` nodes `lower::defs::synthesize_struct_class` built from a
    /// `NAME = Struct.new(:a, :b)`, mapped to their MEMBER list in declaration
    /// order. `analyze` copies it onto `ClassInfo::hidden_ivars`, which is what
    /// makes those slots invisible to `instance_variables` while `Struct`'s own
    /// shared protocol still reaches them by index.
    pub struct_members: crate::compiler::FMap<NodeId, Vec<String>>,
    /// The span of the prism node currently being lowered (innermost last);
    /// `Hir::push` stamps from the top of this stack. Maintained by the
    /// `lower_node` wrapper, empty outside lowering.
    span_stack: Vec<Span>,
    /// Ruby's own parse-time warnings for the files that make up this
    /// program, in the order they were parsed -- see
    /// [`CompileWarning`](crate::diagnostics::CompileWarning). Codegen emits
    /// them into the binary's startup.
    pub warnings: Vec<crate::diagnostics::CompileWarning>,
    /// The main script's `__END__` DATA section: its absolute path and the
    /// byte offset of the first byte AFTER the marker line. `None` when the
    /// script has no `__END__`, which is what leaves `DATA` undefined.
    pub data_section: Option<DataSection>,
    /// Registered source files (`Span::file` indexes here).
    pub files: Vec<SourceFile>,
    /// The ENTRY file -- the one named on the command line. Every other entry
    /// in `files` was pulled in by a `require`, and `require` merges every
    /// file's statements into one arena, so this is the only thing left that
    /// tells the two apart once lowering has finished. `None` for a pathless
    /// source string. Read by `analyze`'s `__FILE__ == $0` fold.
    pub main_file: Option<FileId>,
    /// The file whose source is currently being lowered -- the drivers (the
    /// compiler's `parse_and_lower_with` and its loader) set/restore this
    /// around each file's statements; `None` (source strings with no file
    /// entry: the exception prelude, `eval` bodies) makes every span
    /// `SYNTH`.
    pub lowering_file: Option<FileId>,
    /// Provenance of every `require`/`require_relative`/`load` SPLICE
    /// INSTANCE grafted into this arena, in splice order --
    /// the main file itself is NOT recorded (matching CRuby, where the main
    /// script never enters `$LOADED_FEATURES`). Deliberately per-instance,
    /// not per-canonical-file: `load` re-splices the same file fresh, and
    /// `Ruby::Box` re-executes a file once per box, so an
    /// instance is the unit provenance must track. Nothing downstream
    /// consumes this yet -- it exists so a future `BoxId` becomes a field
    /// flip on `LoadedFile` plus `(box_id, path)`-keyed dedup instead of a
    /// loader rework (see `parse::loader`).
    pub loaded_files: Vec<LoadedFile>,
    /// The compile-time require-search roots (`-I` + `RUBYLIB`), in search
    /// order -- seeded into the runtime `$LOAD_PATH` as COSMETICS: requires
    /// were resolved at compile time, but code that READS the array (rspec's
    /// `RubyProject`) sees what `ruby -I` would show it.
    pub search_roots: Vec<String>,
    /// In-tree `ext/` features (`zeo_abi::is_ext_feature`) whose `require`
    /// fired anywhere in the program -- the set that makes a require-gated
    /// builtin's constant resolvable (`Compiler::resolve_class`'s feature
    /// gate). Whole-program AOT: activation is program-GLOBAL (a `require
    /// "base64"` in any file exposes `Base64` everywhere), a documented
    /// simplification of CRuby's file-ordered visibility that matches how
    /// this loader already splices requires program-wide. See
    /// `activate_feature`.
    pub activated_features: crate::compiler::FSet<String>,
    /// Plain `require "feature"` targets zeo could NOT resolve to a file,
    /// builtin, or shim -- recorded by the loader's resolvability pre-scan.
    /// Their `require` CALL lowers to a runtime `Kernel#require` (which raises
    /// `LoadError`) instead of a loaded-no-op `true`, so a genuinely-missing
    /// feature crashes at its require site and the optional-dependency idiom
    /// (`begin; require "x"; rescue LoadError`) is caught at runtime -- exactly
    /// CRuby's semantics. (A missing `require_relative` stays a compile error.)
    pub unresolvable_requires: crate::compiler::FSet<String>,
    /// `require_relative` CALLS that are lexically inside a `begin` whose
    /// rescue catches `LoadError`, and whose target does not exist -- the
    /// optional-native-half idiom (`begin; require_relative 'geos_c_impl';
    /// rescue LoadError; end`). Keyed by (file, call start offset): a feature
    /// NAME collides across directories (`require_relative "version"` is
    /// everywhere), a call site cannot. These lower to a runtime
    /// `Kernel#require_relative` raising a catchable `LoadError`, exactly as
    /// plain unresolvable `require`s do; every other missing
    /// `require_relative` stays a loud compile error.
    pub optional_require_sites: crate::compiler::FSet<(FileId, u32)>,
    /// `require`/`require_relative` CALLS under a runtime-UNDECIDABLE guard
    /// (`require_relative "hell" if ENV["MT_HELL"]`). CRuby loads the target
    /// only when the guard is true; an eager splice ran it unconditionally --
    /// a recorded divergence (minitest's hell.rb fired its LoadError warn in
    /// every compile). These sites keep their CALL, and the target is
    /// compiled in as a feature unit the runtime require loads when the
    /// guard actually passes. Keyed like `optional_require_sites`.
    pub conditional_require_sites: crate::compiler::FSet<(FileId, u32)>,
    /// Single files to compile as feature units: (owning package, path,
    /// feature name as required). The per-file companion to `unit_demand`'s
    /// per-directory walk, for a conditional require whose one target is
    /// known.
    pub single_unit_demand:
        std::collections::BTreeSet<(Option<String>, std::path::PathBuf, String)>,
    /// Every `FFI::Struct` subclass's computed layout, keyed by LEAF class
    /// name like `ffi_types` -- how a later `attach_function` resolves a
    /// struct passed by value. Source-order like the rest of the FFI table:
    /// the struct's body must lower before the declaration that names it.
    pub ffi_struct_layouts: crate::compiler::FMap<String, FfiStructLayout>,
    /// Class-body `CONST = :symbol` / `CONST = <int>` writes, keyed by LEAF
    /// name -- how an FFI layout in a NESTED class resolves the type/count
    /// vocabulary its enclosing module spelled as constants (ffi-ncurses'
    /// `NCURSES_ATTR_T = :int` ... `layout :attr, NCURSES_ATTR_T`). Same
    /// poison-on-conflict rule as `ffi_types`: a leaf rebound to a DIFFERENT
    /// value goes `None`, and the layout's own honest rejection stands.
    pub ffi_symbol_consts: crate::compiler::FMap<String, Option<String>>,
    /// The integer half of `ffi_symbol_consts`.
    pub ffi_int_consts: crate::compiler::FMap<String, Option<i64>>,
    /// Modules whose `def self.extended(host)` hook runs `host.extend
    /// FFI::Library` (chef's Win32 API indirection), keyed by full cref path.
    /// The value is the hook's flat `host.typedef :src, :alias` stream, which
    /// an `extend <that module>` site replays into its own alias table before
    /// its FFI directives lower. See `lower::ffi::ffi_extender_hook`.
    pub ffi_extenders: crate::compiler::FMap<String, Vec<(String, String)>>,
    /// Modules whose `def self.included(base)` hook runs `base.class_eval`
    /// over a block containing a `layout`, keyed by full cref path. The value
    /// is that block's SOURCE, which an `include <that module>` inside an
    /// `FFI::Struct` body re-parses and lowers in place -- gssapi carries the
    /// layout AND its two readers for every buffer struct in the gem this way.
    /// Source rather than nodes: a prism `Node` is neither `Clone` nor
    /// storable past its `ParseResult`. See `lower::ffi::ffi_layout_hook`.
    pub ffi_layout_hooks: crate::compiler::FMap<String, String>,
    /// How many DEFERRED `ffi_lib` slots the program has minted -- one per
    /// `ffi_lib` statement whose candidates only the running process can
    /// evaluate. The slot number ties that statement's runtime store
    /// (`zeo_rt::ffi::ffi_lib_store`) to every `attach_function` site
    /// lowered under it. See `FfiLib::Deferred`.
    pub ffi_lib_slots: usize,
    /// How many DEFERRED enum slots the program has minted -- one per `enum`
    /// statement whose members only the running process can produce. See
    /// [`FfiType::EnumSlot`].
    pub ffi_enum_slots: usize,
    /// Load paths to compile in WHOLE, as callable units rather than splices --
    /// keyed by owning package name, `None` for the `-I`/main roots. A file
    /// lands here when it computes a `require`/`autoload` target zeo cannot
    /// fold (`ActiveSupport::Autoload#autoload` joins the module name to the
    /// constant and calls `super`), so the only honest answer is to compile in
    /// everything that string could name. See `parse::loader::materialize_units`.
    pub unit_demand: std::collections::BTreeSet<(Option<String>, std::path::PathBuf)>,
    /// The package owning the file currently lowering, `None` for the main
    /// file and the `-I` roots, and that file's own directory -- the pair
    /// `demand_feature_units` records. The directory is what a
    /// `File.expand_path("x", __dir__)` target is relative to, which is how
    /// stdlib and bundler spell a sibling autoload.
    pub lowering_package: Option<String>,
    pub lowering_dir: Option<std::path::PathBuf>,
    /// The materialized units, in discovery order: one file's top-level
    /// statements, under the feature name a `require` would spell. Codegen
    /// emits each as a function and registers it in `zeo_rt::features`.
    pub feature_units: Vec<FeatureUnit>,
    /// Load-path files that could NOT be lowered, as `(feature, absolute,
    /// reason)`. A whole load path is compiled in, so it reaches files the
    /// program may never require; one hitting a lowering gap is recorded here
    /// instead of failing the build, and requiring it raises `LoadError` with
    /// the reason (`zeo_rt::features::install_declined_features`).
    pub declined_units: Vec<(String, String, String)>,
    /// Features named only from inside a method BODY, which zeo therefore does
    /// not load. CRuby loads such a file when the method runs; whole-program
    /// AOT has no runtime loader, so the honest answer is to leave it out and
    /// let the CALL lower to a runtime `Kernel#require`. That answers `false`
    /// if another position did load the feature, and raises `LoadError` if
    /// nothing did. Loading it eagerly instead put it BEFORE the requires the
    /// file itself makes at top level, and dragged every lazy dependency into
    /// the binary.
    pub deferred_requires: crate::compiler::FSet<String>,
    /// How many of the root `Program`'s leading statements came from the
    /// built-in exception classes (`parse::BUILTIN_EXCEPTIONS_RB`), set by
    /// `parse_and_lower_with`. `analyze` marks the classes
    /// those statements register as `is_bootstrap` -- the AOT analogue of
    /// CRuby's "defined before any user program runs" set, which stays
    /// visible inside every `Ruby::Box` (see `Compiler::resolve_class`'s
    /// bootstrap fallback).
    pub builtin_exceptions_len: usize,
    /// How many `Ruby::Box`es the loader allocated -- box ids
    /// run 1..=boxes (0 is the root program). `analyze` creates one
    /// top-level surrogate `ClassInfo` per id.
    pub boxes: u32,
    /// The main file's script encoding as its `Encoding::` CONSTANT spelling
    /// (`"ISO_8859_1"`), set from a `# encoding:` magic comment; `None` is
    /// the UTF-8 default. Governs `__ENCODING__` and the encoding tag of
    /// string literals.
    pub script_encoding: Option<String>,
    /// Set by a `# frozen_string_literal: true` magic comment: every
    /// single-segment (non-interpolated) string literal is then emitted as
    /// its interned, frozen twin. `false` (the default) keeps literals
    /// mutable.
    pub frozen_string_literal: bool,
    /// How many flip-flops have been lowered -- see `HirNode::FlipFlop`. The
    /// counter is per-PROGRAM, not per-file: `require` splices every file into
    /// one arena, so per-file numbering would make two files' first flip-flops
    /// share a latch.
    pub flip_flops: u32,
    /// The `class`/`module` bodies enclosing the statement being lowered,
    /// outermost first, spelled as each definition site wrote them
    /// (`class Net::SMTP` contributes the one entry `"Net::SMTP"`) -- CRuby's
    /// cref chain. Empty means the statement is genuinely top-level; see
    /// [`cvar_is_toplevel`](Self::cvar_is_toplevel) and
    /// [`enclosing_class`](Self::enclosing_class).
    cref_names: Vec<String>,
    /// Whether the statement being lowered is a direct statement of a `class <<
    /// self` body. A `class << self` among them opens the SURROGATE's own
    /// singleton, one level beyond the enclosing-class retagging -- see
    /// [`in_singleton_body`](Self::in_singleton_body).
    in_singleton_body: bool,
    /// How many `def` bodies enclose the node being lowered -- see
    /// [`is_in_def_body`](Self::is_in_def_body).
    def_depth: u32,
    /// Whether the inline-array proxy classes an `FFI::Struct` array field
    /// reads back as have already been synthesized -- see
    /// [`claim_ffi_inline_array_classes`](Self::claim_ffi_inline_array_classes).
    ffi_inline_array_classes: bool,
    /// Every `class`/`module` definition lowered so far, under the FULLY
    /// QUALIFIED name its site spells (`class Error` inside `module Citrus`
    /// records `Citrus::Error`). A `ClassDef` node keeps only the name as
    /// WRITTEN, so an arena scan for one cannot tell citrus's `Citrus::Error`
    /// from a `TomlRB::Error` that is really a `Class.new` value -- and
    /// answering that wrong sends a subclass down the wrong path. See
    /// [`class_defined_in_scope`](Self::class_defined_in_scope).
    class_def_paths: crate::compiler::FSet<String>,
}

/// One compiled-in load-path file -- see `Hir::feature_units`. Its statements
/// are NOT part of the main statement list: codegen emits them as a function
/// the runtime calls when a `require` names `feature`. Registration is
/// unaffected -- the classes it defines are in the dispatch tables from
/// startup, exactly as a spliced file's are.
#[derive(Clone)]
pub struct FeatureUnit {
    /// The name a `require` spells: the path under its load-path root, with
    /// no `.rb`.
    pub feature: String,
    /// The same file's absolute path, with no `.rb` -- the OTHER spelling a
    /// program can build, and the one `File.expand_path("x", __dir__)` (the
    /// `autoload` idiom stdlib and bundler use for a sibling file) produces.
    pub absolute: String,
    pub body: Vec<NodeId>,
}

/// One splice instance -- see `Hir::loaded_files`.
#[derive(Clone)]
pub struct LoadedFile {
    /// Canonicalized (symlink-resolved) path, mirroring CRuby's separate
    /// realpath dedup layer (`load.c`'s `loaded_features_realpaths`).
    pub canonical: std::path::PathBuf,
    /// Index into `loaded_files` of the file whose `require`/`load`
    /// statement pulled this one in; `None` when required directly by the
    /// main file.
    pub required_from: Option<usize>,
    /// The package (`spin.toml` unit) this file belongs to:
    /// `Some(name)` when the file was resolved out of a package's roots, or
    /// pulled in via `require_relative`/`load` FROM a file already
    /// belonging to that package (attribution is inherited -- a package's
    /// internal files are part of the package). `None` for plain `-I`-root
    /// and main-file-relative files. This is the natural box-boundary
    /// candidate for `Ruby::Box` isolation (real box isolation is per
    /// require-graph subtree, and a package is exactly such a subtree).
    pub package: Option<String>,
    /// Always 0 (the root box) for now -- see `Hir::loaded_files`.
    pub box_id: u32,
    /// Compiled in as a FEATURE UNIT (`materialize_units`), not spliced at a
    /// fixed position. A unit has NOT run at program start, so it must not be
    /// seeded into `$LOADED_FEATURES` -- the runtime `require` that loads it
    /// consults that table first, and a seeded unit would answer `false`
    /// without ever running.
    pub is_unit: bool,
}

impl Hir {
    /// Records an FFI type name for the cross-body fallback -- see
    /// [`Hir::ffi_types`]. Redeclaring one to a different type poisons it.
    pub fn declare_ffi_type(&mut self, name: &str, ty: &FfiType) {
        match self.ffi_types.get(name) {
            Some(Some(prev)) if prev == ty => {}
            Some(_) => {
                self.ffi_types.insert(name.to_string(), None);
            }
            None => {
                self.ffi_types.insert(name.to_string(), Some(ty.clone()));
            }
        }
    }

    /// The alias map a class body starts from: every unpoisoned name declared
    /// by an enclosing (or earlier) FFI library. Bodies lower in source order,
    /// so a nested struct sees what the module above it declared.
    pub fn inherited_ffi_types(&self) -> crate::compiler::FMap<String, FfiType> {
        let mut types: crate::compiler::FMap<String, FfiType> = self
            .ffi_types
            .iter()
            .filter_map(|(k, v)| v.clone().map(|t| (k.clone(), t)))
            .collect();
        // Struct layouts join the same table as `Struct` entries. What a
        // bare struct name MEANS depends on the position -- a signature
        // reads it back as `StructRef` (ruby-ffi's by-reference semantics),
        // a layout field keeps the inline by-value struct; see
        // `ffi_type_node`. An explicit typedef of the same name wins.
        for (k, layout) in &self.ffi_struct_layouts {
            types
                .entry(k.clone())
                .or_insert_with(|| FfiType::Struct(layout.clone()));
        }
        // A struct class whose `layout` zeo never saw (DSL-built) still
        // NAMES a struct; the by-reference entry serves every signature
        // position. Last, so a real layout or an explicit typedef wins.
        for k in &self.ffi_struct_classes {
            types
                .entry(k.clone())
                .or_insert_with(|| FfiType::StructRef(k.clone()));
        }
        types
    }

    /// Records that an in-tree `ext/` feature's `require` fired -- exposes
    /// the gated builtin's constant program-wide (see `activated_features`).
    pub fn activate_feature(&mut self, feature: &str) {
        self.activated_features.insert(feature.to_string());
    }

    /// Every node in the arena, for whole-program SYNTACTIC scans -- e.g.
    /// `analyze`'s "is this name ever assigned as a constant anywhere"
    /// check, which needs no tree structure, just the full node set.
    pub fn iter(&self) -> impl Iterator<Item = &HirNode> {
        self.nodes.iter()
    }

    /// The same scan, with each node's id -- for a whole-program search that
    /// must also ask WHERE the hit is, e.g. "does any definition of this
    /// constant lie OUTSIDE the branch this guard controls".
    pub fn iter_with_ids(&self) -> impl Iterator<Item = (NodeId, &HirNode)> {
        self.nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (NodeId(i as u32), n))
    }

    /// Lowers `body` as one more level of cref nesting -- see `cref_names`.
    pub fn in_class_body<T>(&mut self, name: &str, body: impl FnOnce(&mut Self) -> T) -> T {
        self.cref_names.push(name.to_string());
        // An ordinary `class`/`module` written inside a `class << self` body
        // ends the singleton run: ITS `class << self` opens its own singleton,
        // not the surrogate's -- see `in_singleton_body`.
        let out = self.end_singleton_body(body);
        self.cref_names.pop();
        out
    }

    /// Lowers `body` as the statements of a `class << self`, so a `class <<
    /// self` among them can tell that it is the SURROGATE's singleton rather
    /// than an ordinary class's -- see `lower::defs`'s singleton arm.
    pub(crate) fn in_singleton_body<T>(&mut self, body: impl FnOnce(&mut Self) -> T) -> T {
        let saved = std::mem::replace(&mut self.in_singleton_body, true);
        let out = body(self);
        self.in_singleton_body = saved;
        out
    }

    /// Claims the right to emit the inline-array proxy classes: true for the
    /// FIRST `FFI::Struct` in the program with an array field, false for every
    /// one after it. They are one pair of classes per program, and redefining
    /// them per struct would warn on every redefinition.
    pub(crate) fn claim_ffi_inline_array_classes(&mut self) -> bool {
        !std::mem::replace(&mut self.ffi_inline_array_classes, true)
    }

    /// Records that `recv` is a receiver zeo synthesized for a call ruby runs
    /// with an implicit one.
    pub(crate) fn mark_implicit_self_receiver(&mut self, recv: NodeId) {
        self.set_flag(recv, NodeFlag::IMPLICIT_SELF_RECEIVER);
    }

    /// Whether `recv` is a receiver zeo synthesized for a call whose ruby form
    /// is RECEIVERLESS -- the `self.singleton_class` a `class << self` body's
    /// statement is rebound onto. Ruby runs no visibility check on a call with
    /// no receiver, so neither may the rebound form: lita's
    /// `define_deprecated_class_method` is `private` on the singleton's own
    /// singleton and is called from the singleton body beside it.
    ///
    /// A receiver the SOURCE wrote is never in here, so a hand-written
    /// `Foo.singleton_class.some_private_method` still raises, as ruby does.
    pub(crate) fn is_implicit_self_receiver(&self, recv: NodeId) -> bool {
        self.has_flag(recv, NodeFlag::IMPLICIT_SELF_RECEIVER)
    }

    /// Lowers `body` outside any `class << self` run -- see `in_class_body`
    /// and `desugar_singleton_class_defs`, the two constructs that end one.
    pub(crate) fn end_singleton_body<T>(&mut self, body: impl FnOnce(&mut Self) -> T) -> T {
        let saved = std::mem::replace(&mut self.in_singleton_body, false);
        let out = body(self);
        self.in_singleton_body = saved;
        out
    }

    /// Whether the statement being lowered is a direct statement of a `class <<
    /// self` body.
    pub(crate) fn is_in_singleton_body(&self) -> bool {
        self.in_singleton_body
    }

    /// Lowers a `def`'s body with the method-body context marked. A depth,
    /// not a flag: a block or lambda inside the `def` is still inside it,
    /// while a `def` inside a `define_method` block nests one deeper.
    pub(crate) fn in_def_body<T>(&mut self, body: impl FnOnce(&mut Self) -> T) -> T {
        self.def_depth += 1;
        let out = body(self);
        self.def_depth -= 1;
        out
    }

    /// Whether the node being lowered sits inside a `def`'s body -- the one
    /// position the analyze walk never registers a class-body site in (ruby
    /// itself rejects a `class` keyword there), so a desugar that would mint
    /// a `ClassDef` marker must stay a runtime send instead.
    pub(crate) fn is_in_def_body(&self) -> bool {
        self.def_depth > 0
    }

    /// The innermost enclosing `class`/`module`'s name as written, or `None` at
    /// the top level -- what tells `def SMTP.foo` written INSIDE `class SMTP`
    /// (a class method) from `def other.foo` (a per-object singleton).
    pub fn enclosing_class(&self) -> Option<&str> {
        self.cref_names.last().map(String::as_str)
    }

    /// `name` as written, qualified by the cref chain in place RIGHT NOW --
    /// so `Native` inside `module SassC` is `SassC::Native`. Ask it with the
    /// definition's own cref NOT yet pushed (the name is the last component).
    pub(crate) fn cref_path(&self, name: &str) -> String {
        match name.strip_prefix("::") {
            Some(absolute) => absolute.to_string(),
            None if self.cref_names.is_empty() => name.to_string(),
            None => format!("{}::{name}", self.cref_names.join("::")),
        }
    }

    /// The layout hook `name` names as seen from the cref being lowered:
    /// ruby's lexical search, innermost scope first, then the top level. A
    /// `::`-anchored name asks at the top level only. See
    /// [`Hir::ffi_layout_hooks`].
    pub(crate) fn ffi_layout_hook_for(&self, name: &str) -> Option<&String> {
        if let Some(absolute) = name.strip_prefix("::") {
            return self.ffi_layout_hooks.get(absolute);
        }
        for depth in (0..=self.cref_names.len()).rev() {
            let qualified = match depth {
                0 => name.to_string(),
                _ => format!("{}::{name}", self.cref_names[..depth].join("::")),
            };
            if let Some(source) = self.ffi_layout_hooks.get(&qualified) {
                return Some(source);
            }
        }
        None
    }

    /// Records one `class`/`module` definition in `class_def_paths`. Call it
    /// with the definition's OWN cref in place -- i.e. after its body has
    /// lowered and `in_class_body` has popped again.
    pub(crate) fn record_class_def(&mut self, name: &str) {
        let path = self.cref_path(name);
        self.class_def_paths.insert(path);
    }

    /// Marks a module path as an FFI library, and answers whether one already
    /// is. `extend FFI::Library` is written ONCE, in whichever file opens the
    /// module first, but the module is then reopened in others -- sassc's
    /// `SassC::Native` extends in `native.rb` and declares its enums in
    /// `native/sass_value.rb`. Lowering sees one body at a time, so without
    /// this the second file's `enum`/`typedef` are not directives at all.
    ///
    /// Keyed by the FULL cref path, so two unrelated `Native` modules stay
    /// unrelated.
    pub(crate) fn mark_ffi_library(&mut self, path: &str) {
        self.ffi_library_crefs.insert(path.to_string());
    }

    pub(crate) fn is_ffi_library(&self, path: &str) -> bool {
        self.ffi_library_crefs.contains(path)
    }

    /// Records a `class X < FFI::Struct` by leaf name -- see
    /// [`Hir::ffi_struct_classes`].
    pub(crate) fn mark_ffi_struct_class(&mut self, leaf: &str) {
        self.ffi_struct_classes.insert(leaf.to_string());
    }

    /// Whether `leaf` names a class already known to descend from
    /// `FFI::Struct` -- how `class B < A` is recognized as a struct in its own
    /// right. gssapi hides `[]`/`[]=` behind `GssUMStruct < FFI::Struct` and
    /// then declares every real struct against THAT, so nothing in the gem
    /// names `FFI::Struct` directly except the one intermediate.
    pub(crate) fn is_ffi_struct_class(&self, leaf: &str) -> bool {
        self.ffi_struct_classes.contains(leaf)
    }

    /// Whether an already-lowered `class`/`module` definition binds `name` AS
    /// SEEN FROM the cref being lowered: ruby's lexical search, innermost
    /// scope first, then the top level. `::Name` asks at the top level only.
    pub(crate) fn class_defined_in_scope(&self, name: &str) -> bool {
        if let Some(absolute) = name.strip_prefix("::") {
            return self.class_def_paths.contains(absolute);
        }
        (0..=self.cref_names.len()).rev().any(|depth| {
            let candidate = if depth == 0 {
                name.to_string()
            } else {
                format!("{}::{name}", self.cref_names[..depth].join("::"))
            };
            self.class_def_paths.contains(&candidate)
        })
    }

    /// Whether a `@@x` lowered right here resolves to the TOP-LEVEL cref, in
    /// which case Ruby raises `RuntimeError: class variable access from
    /// toplevel` rather than storing anything.
    ///
    /// Only `class`/`module` bodies open a cref. A `def`, a block, a lambda
    /// and a `class << obj` body all leave the enclosing one in place -- so
    /// `Class.new { @@x = 1 }` written at the top level raises, while the
    /// same line inside `class C` stores on `C`. (CRuby reaches the identical
    /// answer by walking `CREF_NEXT` past singleton and eval crefs and
    /// raising when it runs off the end.)
    pub fn cvar_is_toplevel(&self) -> bool {
        self.cref_names.is_empty()
    }
}

impl std::ops::Index<NodeId> for Hir {
    type Output = HirNode;
    fn index(&self, id: NodeId) -> &HirNode {
        &self.nodes[id.0 as usize]
    }
}

/// What kind of Ruby scope a node opens, if any.
///
/// This is the ONE place that enumerates every [`HirNode`] for the purpose of
/// "may a walk descend through this?", the way [`HirNode::for_each_child`] is
/// the one place that enumerates them for "what are its children?". A new
/// variant must declare itself here, and the predicate walks that consume it
/// then match on FIVE variants instead of eighty-one -- so each walk's stop
/// policy is two readable lines, and the differences BETWEEN those policies
/// are visible side by side. They were not, when each walk buried its stops in
/// its own copy of the full match, which is how they drifted apart.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScopeKind {
    /// An ordinary expression or statement: opens nothing.
    None,
    /// A `class`/`module`/`def` body -- a fresh Ruby scope. A `yield`,
    /// `super` or `return` written inside belongs to it, not to whatever
    /// encloses it.
    Definition,
    /// A lambda literal. It catches its own `Signal::Return`, and has no
    /// implicit block of its own (see [`HirNode::Lambda`]).
    Lambda,
    /// A synthesized `attach_function` wrapper body: contains none of the
    /// constructs these walks look for.
    Ffi,
    /// A block literal. Not a stop for any current walk -- a block has neither
    /// its own implicit block nor its own `super` target, so both refer to the
    /// enclosing method -- but it is reached only through the call that
    /// invokes it, which is where its escaping-ness is decided.
    Block,
}

impl HirNode {
    /// See [`ScopeKind`].
    pub fn scope_kind(&self) -> ScopeKind {
        match self {
            HirNode::Ffi(_) => ScopeKind::Ffi,
            HirNode::ClassDef { .. } | HirNode::DefMethod { .. } => ScopeKind::Definition,
            HirNode::Lambda { .. } => ScopeKind::Lambda,
            HirNode::Block { .. } => ScopeKind::Block,
            HirNode::Program(..)
            | HirNode::IntegerLit(..)
            | HirNode::BigIntegerLit { .. }
            | HirNode::RationalLit { .. }
            | HirNode::ImaginaryLit(..)
            | HirNode::FloatLit(..)
            | HirNode::SymbolLit(..)
            | HirNode::NilLit
            | HirNode::BoolLit(..)
            | HirNode::And(..)
            | HirNode::Or(..)
            | HirNode::Defined(..)
            | HirNode::If { .. }
            | HirNode::CaseWhen { .. }
            | HirNode::ArrayLit(..)
            | HirNode::HashLit(..)
            | HirNode::RangeLit { .. }
            | HirNode::StringLit(..)
            | HirNode::RegexpLit(..)
            | HirNode::LocalRead(..)
            | HirNode::LocalWrite(..)
            | HirNode::IvarRead(..)
            | HirNode::IvarWrite(..)
            | HirNode::ClassVarRead(..)
            | HirNode::ClassVarWrite(..)
            | HirNode::ClassRef(..)
            | HirNode::Call { .. }
            | HirNode::New { .. }
            | HirNode::SuperCall { .. }
            | HirNode::Include(..)
            | HirNode::Extend(..)
            | HirNode::Prepend(..)
            | HirNode::ClassMethodPrepend(..)
            | HirNode::DefHook { .. }
            | HirNode::MethodRedefine { .. }
            | HirNode::Refine { .. }
            | HirNode::Using(..)
            | HirNode::While { .. }
            | HirNode::Loop { .. }
            | HirNode::For { .. }
            | HirNode::Break(..)
            | HirNode::Next(..)
            | HirNode::Redo
            | HirNode::MultiWrite { .. }
            | HirNode::Eval(..)
            | HirNode::BoxScope { .. }
            | HirNode::BoxHandle(..)
            | HirNode::Return(..)
            | HirNode::Yield(..)
            | HirNode::BlockGiven
            | HirNode::SelfRef
            | HirNode::Raise(..)
            | HirNode::CaseIn { .. }
            | HirNode::MatchPredicate { .. }
            | HirNode::MatchRequired { .. }
            | HirNode::Begin { .. }
            | HirNode::Retry
            | HirNode::GlobalRead(..)
            | HirNode::GlobalWrite(..)
            | HirNode::QualifiedConstRead(..)
            | HirNode::ConstReadOrNil(..)
            | HirNode::ConstWrite { .. }
            | HirNode::DynConstRead { .. }
            | HirNode::DynConstWrite { .. }
            | HirNode::PreExec(..)
            | HirNode::AliasGlobal(..)
            | HirNode::Undef(..)
            | HirNode::ClassMethodUndef(..)
            | HirNode::AliasMethod { .. }
            | HirNode::MethodVisibility { .. }
            | HirNode::ClassMethodVisibility { .. }
            | HirNode::ModuleFunction(..)
            | HirNode::ConstantVisibility { .. }
            | HirNode::LastMatchRef(..)
            | HirNode::Seq(..)
            | HirNode::FlipFlop { .. } => ScopeKind::None,
        }
    }
}

impl std::ops::IndexMut<NodeId> for Hir {
    fn index_mut(&mut self, id: NodeId) -> &mut HirNode {
        &mut self.nodes[id.0 as usize]
    }
}

impl Hir {
    pub fn push(&mut self, node: HirNode) -> NodeId {
        self.index_const_write(&node);
        self.nodes.push(node);
        self.spans
            .push(self.span_stack.last().copied().unwrap_or(Span::SYNTH));
        self.flags.push(0);
        NodeId((self.nodes.len() - 1) as u32)
    }

    /// Records a `ConstWrite` in [`const_writes`](Self::const_writes). Every
    /// one in the program is born through a `push`, so hooking the two push
    /// paths keeps the map complete; nothing rewrites a `ConstWrite`'s `scope`
    /// or `name` in place afterwards (`rename` visits only its `value`).
    fn index_const_write(&mut self, node: &HirNode) {
        if let HirNode::ConstWrite { scope, name, value } = node {
            let key = match scope {
                Some(s) => format!("{s}::{name}"),
                None => name.clone(),
            };
            self.const_writes.entry(key).or_default().push(*value);
        }
    }

    /// The values assigned to the constant spelled `name` so far, in push
    /// order -- empty when nothing lowered yet assigns it. See
    /// [`const_writes`](Self::const_writes) for what "spelled" means.
    pub fn const_write_values(&self, name: &str) -> &[NodeId] {
        self.const_writes.get(name).map_or(&[], Vec::as_slice)
    }

    /// Records `flag` about `id` -- see [`NodeFlag`].
    pub fn set_flag(&mut self, id: NodeId, flag: NodeFlag) {
        let slot = &mut self.flags[id.0 as usize];
        // The bit answers "was it already set?" in one load, which is what
        // keeps `block_bodied_def_list` free of duplicates without scanning it
        // -- the sets these flags replaced deduped for free.
        let fresh = *slot & flag.0 == 0;
        *slot |= flag.0;
        if fresh && flag == NodeFlag::BLOCK_BODIED_DEF {
            self.block_bodied_def_list.push(id);
        }
    }

    /// Whether `flag` was recorded about `id` -- see [`NodeFlag`].
    pub fn has_flag(&self, id: NodeId, flag: NodeFlag) -> bool {
        self.flags[id.0 as usize] & flag.0 != 0
    }

    /// Every [`NodeFlag::BLOCK_BODIED_DEF`] node, in push order.
    pub fn block_bodied_defs(&self) -> &[NodeId] {
        &self.block_bodied_def_list
    }

    /// Records that the `DefMethod` at `def` is an `alias` of `original` --
    /// the one fact a same-body alias's cloned node cannot carry itself, and
    /// what `Method#original_name` answers. Keyed by node rather than added
    /// to `DefMethod` because only the handful of alias clones ever have it.
    pub fn record_alias_origin(&mut self, def: NodeId, original: impl Into<String>) {
        self.alias_origins.insert(def, original.into());
    }

    /// The name the `DefMethod` at `def` was born under, if it is an alias.
    pub fn alias_origin(&self, def: NodeId) -> Option<&str> {
        self.alias_origins.get(&def).map(String::as_str)
    }

    /// [`push`](Self::push), but inheriting `origin`'s provenance instead of
    /// the enclosing statement's. For a node SYNTHESIZED from another one --
    /// the `DefMethod` an `alias` clones, the accessors `attr_reader`
    /// expands to -- the source location Ruby reports is the original's, not
    /// wherever the expansion happens to sit.
    pub fn push_from(&mut self, node: HirNode, origin: NodeId) -> NodeId {
        let span = self.spans[origin.0 as usize];
        self.index_const_write(&node);
        self.nodes.push(node);
        self.spans.push(span);
        self.flags.push(0);
        NodeId((self.nodes.len() - 1) as u32)
    }

    /// Every node in the arena, in push order -- for whole-program scans
    /// that don't care about tree structure (e.g. codegen's
    /// super-reachability analysis; `const_is_assigned` is the precedent).
    pub fn all_nodes(&self) -> &[HirNode] {
        &self.nodes
    }

    /// `all_nodes`' ids, in the same push order -- for whole-program scans
    /// that need the id alongside the node (e.g. telling a scope-claimed
    /// `DefMethod` from a runtime-defined one).
    pub fn node_ids(&self) -> impl Iterator<Item = NodeId> + use<> {
        (0..self.nodes.len() as u32).map(NodeId)
    }

    /// The provenance of `id` -- `None` for a synthetic node (see `Span`).
    pub fn span(&self, id: NodeId) -> Option<Span> {
        self.spans[id.0 as usize].known()
    }

    /// Records that the file currently lowering computes a `require`/`autoload`
    /// target, so its whole load path must be compiled in as units. Keyed by
    /// the file's owning package, which bounds the walk to that gem.
    pub fn demand_feature_units(&mut self) {
        let package = self.lowering_package.clone();
        let dir = self.lowering_dir.clone().unwrap_or_default();
        tracing::debug!(?package, ?dir, file = ?self.lowering_file, "demand_feature_units");
        self.unit_demand.insert((package, dir));
    }

    /// Registers a source file for span provenance; the caller then sets
    /// `lowering_file` while that file's statements lower.
    pub fn add_file(
        &mut self,
        name: impl Into<String>,
        source: impl Into<std::sync::Arc<str>>,
    ) -> FileId {
        let source = source.into();
        let frozen_string_literal = magic_frozen_string_literal(&source);
        let mut line_starts = vec![0u32];
        for (i, b) in source.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push((i + 1) as u32);
            }
        }
        self.files.push(SourceFile {
            name: name.into(),
            source,
            frozen_string_literal,
            line_starts,
        });
        FileId((self.files.len() - 1) as u32)
    }

    /// The entry file's name AS GIVEN on the command line -- the same string
    /// `__FILE__` answers with inside it (see `main_file`).
    pub fn main_file_name(&self) -> Option<&str> {
        let id = self.main_file?;
        Some(self.files[id.0 as usize].name.as_str())
    }

    /// Enters/leaves the span frame for one prism node -- called only by the
    /// `lower_node` wrapper, paired push/pop.
    pub(crate) fn push_span(&mut self, span: Span) {
        self.span_stack.push(span);
    }
    pub(crate) fn pop_span(&mut self) {
        self.span_stack.pop();
    }

    /// The span a `push` would stamp right now -- the prism node currently
    /// lowering. For a lowering that hands a SYNTHESIZED SOURCE STRING to
    /// `parse_and_lower_into`: the offsets that parse produces index that
    /// string, not the file, so a node built from them must be re-stamped with
    /// the position the user actually wrote. See [`Self::set_span`].
    pub(crate) fn current_span(&self) -> Span {
        self.span_stack.last().copied().unwrap_or(Span::SYNTH)
    }

    /// Corrects `id`'s provenance after the fact. See [`Self::current_span`].
    pub(crate) fn set_span(&mut self, id: NodeId, span: Span) {
        self.spans[id.0 as usize] = span;
    }

    /// Every node lowered so far, for the rare pass that must ask a
    /// whole-arena question mid-lowering (see `parse::const_is_assigned`).
    pub fn nodes(&self) -> &[HirNode] {
        &self.nodes
    }

    /// Retroactively overrides an already-lowered `DefMethod`'s visibility --
    /// used by `parse::lower_class_body_statement` for `private`/`public`/
    /// `protected :name` (marking an already-lowered method by name) and the
    /// `private def name; ... end` idiom (the `def` is lowered normally
    /// first, then its visibility corrected). Panics if `id` isn't a
    /// `DefMethod` -- every call site already confirmed the node shape
    /// before calling.
    pub fn set_method_visibility(&mut self, id: NodeId, visibility: Visibility) {
        let HirNode::DefMethod { visibility: v, .. } = &mut self.nodes[id.0 as usize] else {
            panic!("set_method_visibility: node isn't a DefMethod");
        };
        *v = visibility;
    }

    /// Retroactively marks an already-lowered `DefMethod` as a CLASS method
    /// -- used by `parse::lower_class_body_statement`'s `class << self`
    /// recognizer: the nested body is lowered exactly like an ordinary class
    /// body first (so `attr_reader`/`private`/etc. inside it still work),
    /// then every resulting `def` is corrected to `is_class_method: true`
    /// (real Ruby: everything defined inside `class << self` becomes a
    /// method on the class itself, not an instance method). Panics if `id`
    /// isn't a `DefMethod` -- the caller already rejects any other statement
    /// shape appearing inside `class << self` (zeo limitation).
    pub fn set_method_is_class_method(&mut self, id: NodeId) {
        let HirNode::DefMethod {
            is_class_method, ..
        } = &mut self.nodes[id.0 as usize]
        else {
            panic!("set_method_is_class_method: node isn't a DefMethod");
        };
        *is_class_method = true;
    }

    /// Corrects an `AliasMethod` to a class-method alias, for an `alias` inside
    /// `class << self` (see `HirNode::AliasMethod::is_class_method`). Panics if
    /// `id` isn't an `AliasMethod` -- the caller only reaches this for one.
    pub fn set_alias_is_class_method(&mut self, id: NodeId) {
        let HirNode::AliasMethod {
            is_class_method, ..
        } = &mut self.nodes[id.0 as usize]
        else {
            panic!("set_alias_is_class_method: node isn't an AliasMethod");
        };
        *is_class_method = true;
    }

    /// A fresh, arena-wide-unique synthetic identifier, for a compiler-
    /// introduced hidden local that never appears in real Ruby source (e.g.
    /// binding a compound-assignment target's receiver/index expression to a
    /// name exactly once, so `obj.attr += 1` / `arr[i] += 1` don't
    /// double-evaluate a side-effecting receiver -- see `lower_call_operator_write`'s
    /// docs). `self.nodes.len()` strictly increases with every `push`, so
    /// calling this before pushing anything for the current desugar gives a
    /// suffix no earlier OR later desugar in the same file can ever collide
    /// with.
    /// Every prefix [`gensym`](Hir::gensym) is called with, plus the
    /// destructuring-param slot that is built without it. See
    /// [`is_internal_local`], which is what keeps them out of
    /// `local_variables`.
    pub const INTERNAL_LOCAL_PREFIXES: &'static [&'static str] = &[
        "__recv", "__idx", "__asgn", "__mval", "__destr", "__cscope", "__resc", "__loop",
    ];

    pub fn gensym(&self, prefix: &str) -> String {
        debug_assert!(
            Self::INTERNAL_LOCAL_PREFIXES.contains(&prefix),
            "a new hidden-local prefix must be listed in INTERNAL_LOCAL_PREFIXES, \
             or `local_variables` will report it as a Ruby local"
        );
        format!("{prefix}{}", self.nodes.len())
    }

    /// Whether the final binary must link the PRISM-BACKED runtime variant,
    /// or can stay lean (parser-free) -- what `backend::Runtime` decides
    /// from.
    ///
    /// Two things reach prism, and they are separate questions. The runtime
    /// eval VM is one ([`Hir::uses_runtime_eval`]). The other is `require
    /// "prism"`, whose `ext-prism` module calls the same C library's
    /// serialize entry points: the lean variant does not link it, so such a
    /// program would otherwise fail at LINK time, where no message can
    /// explain itself.
    pub fn needs_prism_runtime(&self) -> bool {
        self.activated_features.contains("prism")
            || self.uses_runtime_eval()
            || self.mentions_rubyvm_parser()
    }

    /// Whether the program names a `RubyVM` surface whose body parses at
    /// runtime (`AbstractSyntaxTree.parse`, `InstructionSequence.compile`) --
    /// those link the prism runtime exactly like a dynamic `eval` does.
    fn mentions_rubyvm_parser(&self) -> bool {
        self.nodes.iter().any(|node| match node {
            HirNode::ClassRef(n) => n == "RubyVM",
            HirNode::QualifiedConstRead(scope, n) => {
                n == "RubyVM" || scope == "RubyVM" || scope.starts_with("RubyVM::")
            }
            _ => false,
        })
    }

    /// Whether this program can reach the RUNTIME eval VM.
    ///
    /// Only `Kernel#eval` and string-form `instance_eval` funnel into
    /// `zeo_rt::eval_value`/`eval_string`. A literal `eval("...")` does not
    /// count: lowering already spliced it into the arena as `HirNode::Eval`,
    /// so any surviving `Call` named `eval` is the dynamic form. Block-form
    /// `instance_eval { ... }` runs a real block and carries it in `block`,
    /// so a POSITIONAL argument marks the string form.
    ///
    /// This scans the whole arena rather than traversing from the roots, so
    /// it also catches eval sites inside spliced files and method bodies.
    /// Over-approximation is safe: a false positive only links the larger
    /// runtime. A miss -- `send(name, src)` with a computed name, which no
    /// static analysis can see -- raises the runtime's own
    /// `NotImplementedError` naming `--features eval-vm`, never silent
    /// wrong output.
    pub fn uses_runtime_eval(&self) -> bool {
        self.nodes.iter().any(|node| match node {
            HirNode::Call { name, args, .. } => match name.as_str() {
                // Any `eval`, receiver or not: `Binding#eval` runs its source
                // through the same VM, and a Binding is an ordinary value a
                // call site can hold in anything.
                "eval" => true,
                "instance_eval" | "class_eval" | "module_eval" => !args.is_empty(),
                // A reflective `send(:eval, src)` is an eval site too, and a
                // LITERAL method symbol is one static analysis can see.
                "send" | "__send__" | "public_send" => args
                    .first()
                    .and_then(|a| self.sent_name(a.node_id()))
                    .is_some_and(|n| {
                        matches!(n, "eval" | "instance_eval" | "class_eval" | "module_eval")
                    }),
                _ => false,
            },
            _ => false,
        })
    }

    /// Whether the program can observe a `:call`/`:return` event -- it names
    /// `TracePoint` or calls `set_trace_func` anywhere.
    ///
    /// Those two events are the only thing a devirtualized accessor stops
    /// producing (`codegen::params::emit_accessor_trampoline` reaches the
    /// field with no frame and no callee at all), so codegen keeps the
    /// ordinary call shape for a program that could watch for them -- the
    /// same "the instrumentation is ABSENT, not branched on" model
    /// `codegen::coverage_active` uses.
    ///
    /// A plain scan of the whole arena, matching `uses_runtime_eval`: it must
    /// see spliced `require`d files and method bodies too. Over-approximation
    /// is the safe direction (a program that merely mentions the constant
    /// just keeps today's accessors), and the honest miss -- reaching
    /// `TracePoint` through a computed name -- costs a `:call` event on
    /// one-line accessors, not wrong output.
    pub fn uses_call_tracing(&self) -> bool {
        self.nodes.iter().any(|node| match node {
            // `TracePoint.new` lowers to `New`, not a `Call` on a `ClassRef` --
            // the receiver never survives as its own node, so matching only
            // constant reads would miss the single commonest spelling.
            HirNode::New { class_name: n, .. }
            | HirNode::ClassRef(n)
            | HirNode::QualifiedConstRead(_, n)
            | HirNode::ConstReadOrNil(_, n) => n == "TracePoint",
            HirNode::Call { name, args, .. } => {
                name == "set_trace_func"
                    || args
                        .first()
                        .and_then(|a| self.sent_name(a.node_id()))
                        .is_some_and(|n| n == "set_trace_func")
            }
            _ => false,
        })
    }

    /// Whether the program names `Ractor` anywhere -- the emission switch for
    /// the moved-object guards (`codegen::call::builtins`' `_checked` container
    /// twins, the inlined-accessor husk check): only a program that can reach
    /// `Ractor` can ever poison an object with `send(obj, move: true)`, so
    /// everything else keeps the guard-free fast paths. A plain whole-arena
    /// scan like [`uses_call_tracing`], and over-approximation is the safe
    /// direction (a program that merely mentions the constant just carries
    /// the cheap guards).
    pub fn uses_ractor(&self) -> bool {
        self.nodes.iter().any(|node| match node {
            HirNode::New { class_name: n, .. }
            | HirNode::ClassRef(n)
            | HirNode::QualifiedConstRead(_, n)
            | HirNode::ConstReadOrNil(_, n) => n == "Ractor" || n.starts_with("Ractor::"),
            _ => false,
        })
    }

    /// The literal method name a `send`-family call names, if it is a symbol
    /// or string literal -- `None` for a computed one.
    pub fn sent_name(&self, first_arg: NodeId) -> Option<&str> {
        match &self[first_arg] {
            HirNode::SymbolLit(n) => Some(n),
            HirNode::StringLit(parts) => match parts.as_slice() {
                [StrPart::Lit(n)] => Some(n),
                _ => None,
            },
            _ => None,
        }
    }

    /// Whether the program can ask a `Proc` for its `#binding` -- any call
    /// named `binding` WITH a receiver, or a `send(:binding)`. Codegen only
    /// captures a block's defining scope when this holds, so a program that
    /// never reflects on a Proc that way pays nothing (see
    /// `codegen::captures::binding_scope_names`). Memoized: every method
    /// scope asks, and the answer is a whole-program property.
    pub fn uses_proc_binding(&self) -> bool {
        *self.proc_binding.get_or_init(|| {
            self.nodes.iter().any(|node| match node {
                HirNode::Call {
                    receiver,
                    name,
                    args,
                    ..
                } => {
                    (name == "binding" && receiver.is_some() && args.is_empty())
                        || (matches!(name.as_str(), "send" | "__send__" | "public_send")
                            && args.len() == 1
                            && self.sent_name(args[0].node_id()) == Some("binding"))
                }
                _ => false,
            })
        })
    }
}

/// One element of an `ArrayLit` -- a plain value, or a `*expr` splat whose
/// contents are flattened in at runtime (its length isn't known until then,
/// so this can't just be another plain element).
#[derive(Clone)]
pub enum ArrayElem {
    Single(NodeId),
    Splat(NodeId),
}

impl ArrayElem {
    /// The single child node this element wraps (`Single`'s value or `Splat`'s
    /// operand) -- lets HIR walkers visit an arg list without re-matching the
    /// variant. The `Single`/`Splat` distinction only matters at codegen.
    pub fn node_id(&self) -> NodeId {
        let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = self;
        *n
    }
}

/// A method/block's declared parameter list -- mirrors `ParametersNode`'s own
/// grouping directly (Ruby's grammar already enforces required->optional->
/// rest->post->keyword->keyword_rest->block ordering, so grouping by kind
/// loses nothing, and it maps 1:1 onto what `parse/mod.rs::lower_params`
/// reads off `ruby_prism::ParametersNode`).
///
/// `default_ids()` below is the one place `analyze::collect_ivars`/
/// `analyze::locals::track_extra`/`codegen::hoisting`'s per-method scans walk
/// INTO a default-value expression for `@ivar`/local references -- otherwise
/// a default that reads an ivar/local nowhere else referenced (e.g. `def
/// f(x: @only_here)`) could hit a "no such field" codegen error instead of
/// working.
#[derive(Clone, Default)]
pub struct Params {
    pub required: Vec<String>,
    /// Parenthesized DESTRUCTURING params (`|a, (b, c), d|`, `|(a, *r)|`,
    /// nested `|(a, (b, c))|`). Ruby lets any positional slot be a
    /// parenthesized target list that splits the value bound to it, which is
    /// exactly a multi-assignment of that slot -- so it is lowered as one
    /// rather than given its own binding machinery.
    ///
    /// `lower_params` names each such slot internally (`__destr_<i>`, which
    /// is what lands in `required`/`post`, keeping every arity rule --
    /// counting, auto-splat, `Proc#arity` -- working on plain names), and
    /// records here the `LocalRead` of that slot plus the target group to
    /// split it into. The two param-binding sites (`emit_prologue` for
    /// methods, `emit_proc_param_bindings` for blocks) replay these through
    /// the ordinary `emit_multi_write` immediately after binding.
    pub destructures: Vec<(NodeId, MultiTargetGroup)>,
    /// Each default value expression is evaluated LAZILY -- only when its
    /// argument is actually omitted at the call site -- so codegen emits it
    /// inside the callee's own prologue, never eagerly at every call site.
    pub optional: Vec<(String, NodeId)>,
    /// `None`: no `*` at all. `Some(None)`: an anonymous `*` (collects and
    /// discards the extra positional args). `Some(Some(name))`: `*name`.
    pub rest: Option<Option<String>>,
    /// The `rest` above came from a TRAILING COMMA (`|a,|`), not from a written
    /// `*`. It turns on auto-splat and discards what follows the named
    /// parameters -- but it is not part of the SIGNATURE: `proc { |x,| }.arity`
    /// is 1, `#parameters` reports only `x`, and `lambda { |a,| }` still
    /// refuses two arguments.
    pub implicit_rest: bool,
    /// Required params that appear AFTER a splat (`def f(a, *b, c)` -- `c`
    /// is a `post`; real Ruby allows this, and it's a distinct binding rule
    /// from `required` since its position is anchored from the END of the
    /// argument list, not the start).
    pub post: Vec<String>,
    pub keywords: Vec<KeywordParam>,
    /// Same `None`/`Some(None)`/`Some(Some(name))` shape as `rest`.
    pub keyword_rest: Option<Option<String>>,
    /// `**nil` -- the method accepts NO keywords at all. Distinct from
    /// "no `keyword_rest`": without a `**kwrest` a callee that declares no
    /// keywords silently takes trailing ones as a positional options Hash, and
    /// `**nil` (ruby 3.0) exists to forbid exactly that conversion. Passing
    /// keywords anyway is `ArgumentError: no keywords accepted`, raised BEFORE
    /// the arity check -- `nokw(1, 2, 3, b: 4)` reports the keywords, not the
    /// count. An EMPTY `**{}` splat passes nothing, so it does not raise.
    pub no_keywords: bool,
    /// `&blk` / anonymous `&` -- same `None`/`Some(None)`/`Some(Some(name))`
    /// shape as `rest`/`keyword_rest` again. Bound to `Nil` when the method
    /// is called with no block (real Ruby: an unyielded `&blk` is `nil`, not
    /// absent) -- see `codegen::params::emit_prologue`. Bare `...`
    /// forwarding (which implies a block too, among other things) is still a
    /// clean lowering error -- see `parse/mod.rs::lower_params`'s docs.
    pub block: Option<Option<String>>,
    /// BLOCK-LOCAL declarations -- the names after the `;` in `|x; sum|`.
    /// Always empty for a method's `Params`; the syntax exists only on a
    /// block.
    ///
    /// These are not parameters. Nothing binds to them from the argument
    /// list, so they take no signature slot and count toward no arity rule.
    /// They are fresh locals, re-initialized to `nil` on EVERY invocation.
    /// That is what makes them more than a naming convention:
    ///
    /// ```ruby
    /// total = 42
    /// [1, 2, 3].each { |x; total| total = (total || 0) + x }
    /// total  # => 42 -- never written, and never accumulated
    /// ```
    ///
    /// They do appear in `bound_names`, which is what shadows an enclosing
    /// local correctly. `captures::own_param_names`, `Ctx::in_proc` and
    /// hoisting all read that one enumeration.
    pub block_locals: Vec<String>,
    /// The block's IMPLICIT block-locals: names in prism's block-scope local
    /// table that are first-assigned inside the body (so not parameters and not
    /// the explicit `;`-block-locals above). Ruby re-initializes them to nil on
    /// EVERY invocation, so a conditional first-assignment (`x = v if cond`)
    /// must not leak into the next iteration. The inline `.times`/range-each
    /// splice (which shares the enclosing Rust scope rather than allocating a
    /// closure) resets them per iteration; escaping blocks reset via their
    /// own-locals prelude instead. Empty for methods.
    pub implicit_block_locals: Vec<String>,
}

/// A method's visibility, as of the point in the class body where its `def`
/// was lowered (`private`/`public`/`protected` with no arguments switches the
/// DEFAULT for every subsequent `def` in the same class body -- see
/// `parse::lower_class_body`'s docs) or set retroactively by a same-named
/// `private`/`public`/`protected :name` / `private def name; ... end` form.
/// Enforced at `codegen::call::dispatch`'s Path 1 site and `zeo_rt::send`'s
/// Path 2 dispatch -- see their docs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Visibility {
    #[default]
    Public,
    Private,
    /// Callable with an explicit receiver only from within a method whose
    /// OWN receiver class is ancestor-related to the method's defining
    /// class (real Ruby's actual rule -- e.g. `def ==(other); x == other.x;
    /// end` calling a `protected` `x` on `other`, another instance of the
    /// same class).
    Protected,
}

#[derive(Clone)]
pub enum KeywordParam {
    Required(String),
    /// Same lazy-default-evaluation contract as `Params::optional`.
    Optional(String, NodeId),
}

impl Params {
    /// Every default-value expression this `Params` declares (positional
    /// `optional` + keyword-optional) -- the one place all three per-method
    /// scans (ivar collection, local-type tracking, hoisting's local
    /// collection) need to additionally walk into, alongside the method's
    /// own body, since a default can reference `@ivar`s/locals exactly like
    /// an ordinary statement can (see this struct's docs).
    pub fn default_ids(&self) -> Vec<NodeId> {
        let mut ids: Vec<NodeId> = self.optional.iter().map(|(_, d)| *d).collect();
        for kw in &self.keywords {
            if let KeywordParam::Optional(_, d) = kw {
                ids.push(*d);
            }
        }
        ids
    }

    /// Every NAME this `Params` binds in the method's scope (required,
    /// optional, named rest/kwrest/block, post, keywords) -- what
    /// `codegen::hoisting` consults so a REASSIGNED parameter is rebound
    /// from its already-bound value (`let mut x = x;`) instead of shadowed
    /// by the nil-defaulted hoisting declaration (a pre-existing
    /// silent wrongness surfaced by bare-`super` forwarding, where
    /// `def f(name); name = name.upcase; super; end` must forward the
    /// reassigned value).
    pub fn bound_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.required.clone();
        names.extend(self.optional.iter().map(|(n, _)| n.clone()));
        if let Some(Some(n)) = &self.rest {
            names.push(n.clone());
        }
        names.extend(self.post.iter().cloned());
        for kw in &self.keywords {
            match kw {
                KeywordParam::Required(n) | KeywordParam::Optional(n, _) => names.push(n.clone()),
            }
        }
        if let Some(Some(n)) = &self.keyword_rest {
            names.push(n.clone());
        }
        if let Some(Some(n)) = &self.block {
            names.push(n.clone());
        }
        // The names a destructuring param binds are nested inside its target
        // group, not in `required` (which holds only the internal slot name)
        // -- but they are every bit as much parameters of this scope, so
        // hoisting has to declare them and capture analysis has to treat
        // them as the block's OWN names rather than enclosing-scope captures.
        names.extend(self.destructured_names());
        // Block-locals (`|x; sum|`) bind nothing from the argument list, but
        // they are unambiguously this block's OWN names -- which is the
        // question every `bound_names` caller is actually asking. Including
        // them here is what makes them shadow an enclosing `sum` instead of
        // being classified as a capture of it. See the field's docs.
        names.extend(self.block_locals.iter().cloned());
        names
    }

    /// Just the names bound INSIDE destructuring params (`b`/`c` of
    /// `|a, (b, c)|`) -- the part of `bound_names` that the Rust fn signature
    /// does NOT bind, since only the `__destr_<i>` slot has a signature
    /// parameter. Callers that mean "the names arriving as real Rust
    /// parameters" want `bound_names` minus this.
    pub fn destructured_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        for (_, group) in &self.destructures {
            group.collect_local_names(&mut names);
        }
        names
    }
}

/// One element of a keyword-argument list / `{ }` literal, in SOURCE ORDER --
/// the keyword-position analogue of [`ArrayElem`]. `Pair` is a literal
/// `k: v` / `k => v`; `DoubleSplat` is a `**expr` whose runtime hash is merged
/// in AT THIS POSITION (its keys aren't known until runtime). Ordering is
/// observable because Ruby's Hash is insertion-ordered, so `f(**a, c: 1)` and
/// `f(c: 1, **a)` build different hashes -- which is why a call's keywords
/// can't be split into a separate `pairs` list and `**` slot. One type serves
/// `Call`, `New`, `HashLit`, and (via a trailing `HashLit`) `Yield`.
#[derive(Clone)]
pub enum KwArg {
    Pair(NodeId, NodeId),
    DoubleSplat(NodeId),
}

impl KwArg {
    /// The child node ids this element references, for HIR walkers (a `Pair`'s
    /// key+value, or a `DoubleSplat`'s single expression) -- so every pass can
    /// visit a `kwargs` list uniformly without re-matching the variant.
    pub fn node_ids(&self) -> impl Iterator<Item = NodeId> + use<> {
        let (a, b) = match *self {
            KwArg::Pair(k, v) => (k, Some(v)),
            KwArg::DoubleSplat(n) => (n, None),
        };
        std::iter::once(a).chain(b)
    }
}

/// The `cause:` keyword on a `raise`, as a genuine THREE-state value.
///
/// CRuby distinguishes these with a `Qundef`/`Qnil`/value sentinel
/// (`rb_f_raise` -> eval.c:740) because the first two mean OPPOSITE things:
/// an omitted `cause:` chains automatically from `$!`, while an explicit
/// `cause: nil` suppresses that chaining. Modeling this as a plain
/// `Option<NodeId>` invites exactly one bug -- lowering `cause: nil` to
/// `None` -- which would silently chain anyway, so the states are named.
#[derive(Debug, Clone, PartialEq)]
pub enum RaiseCause {
    /// No `cause:` written: chain automatically from `$!`.
    Absent,
    /// `cause: <expr>` was written. The expression may still evaluate to
    /// nil, which SUPPRESSES chaining rather than requesting it.
    Explicit(NodeId),
}

/// The `cause:` expression of a `raise`, if one was written.
///
/// Every HIR walker must visit it alongside the positional operands -- it is
/// an ordinary expression that can read locals, capture them into a block, or
/// name constants. Exposed here so no walker has to re-match [`RaiseCause`]
/// and risk quietly forgetting the node.
pub fn raise_cause_node(cause: &RaiseCause) -> Option<NodeId> {
    match cause {
        RaiseCause::Absent => None,
        RaiseCause::Explicit(id) => Some(*id),
    }
}

/// Which last-match special a `LastMatchRef` reads -- see that variant's
/// docs. All of them derive from the one `$~` slot.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum LastMatch {
    /// `$~` -- the MatchData itself.
    Data,
    /// `$1`..`$9` (and `$&`, which is group 0).
    Group(usize),
    /// `` $` `` -- the text before the match.
    Pre,
    /// `$'` -- the text after it.
    Post,
    /// `$+` -- the highest-numbered group that PARTICIPATED in the match
    /// (skipping declared-but-unmatched ones), or nil if only group 0 did.
    LastGroup,
}

/// A `case/in` pattern -- a small, directly-recursive tree, deliberately NOT
/// reusing `HirNode`/`NodeId` for every position: a pattern's leaves have
/// fundamentally different semantics than an expression's (a bare
/// identifier BINDS in pattern position, it doesn't READ), so giving it its
/// own type avoids overloading `HirNode::LocalRead`/`LocalWrite` with a
/// meaning they don't otherwise have. Literal sub-expressions (a pinned
/// value, a range endpoint, a plain literal/expression matched via `===`)
/// still lower through the ordinary `NodeId`/`lower_node` path -- only the
/// PATTERN SHAPE itself is bespoke.
#[derive(Clone)]
pub enum Pattern {
    /// A bare identifier (`x`, `_`, `_foo`) -- always matches, binding the
    /// scrutinee to this name in the enclosing method scope (exactly like an
    /// `if`/`case`-branch local -- no new Ruby scope is introduced).
    Bind(String),
    /// A literal or arbitrary expression, matched via `RubyValue::rb_eq`
    /// against the scrutinee -- the same value-equality escape hatch
    /// `case/when`'s value matching already uses (see `HirNode::CaseWhen`'s
    /// docs), not real Ruby's fully general `#===` protocol. `nil`/`true`/
    /// `false`/`Integer`/`Symbol`/`String` literals and any other plain
    /// expression all fall here.
    Value(NodeId),
    /// `^x` / `^(expr)` -- matched via `RubyValue::rb_eq` against the
    /// ALREADY-BOUND value `x`/`expr` evaluates to (never introduces a new
    /// binding, unlike `Bind`).
    Pin(NodeId),
    /// A bare constant used as a pattern with no capture (`in Integer`,
    /// `in SomeClass`) -- an `is_a?`-style ancestry/tag check with no
    /// binding. See `codegen::patterns::emit_class_check`'s docs for how
    /// this resolves both built-in primitive names (`Integer`/`String`/
    /// `Symbol`/`Array`/`Hash`/`Range`/`Proc`/`NilClass`/`TrueClass`/
    /// `FalseClass`, checked via a runtime tag) and user-defined classes
    /// (checked via the same linearized `ancestors` list `is_a?`/`super`
    /// already consult).
    ClassCheck(String),
    /// `1..10` / `..5` / `1..` as a pattern -- "does the scrutinee fall
    /// inside this range", not `rb_eq`. Scope-cut, matching `codegen::loops`'
    /// existing `for`-in-`Range` restriction: only a statically/dynamically
    /// `Int`-valued scrutinee is supported (checked via `as_int_unchecked`,
    /// not a general `Comparable`-based `#cover?`).
    Range {
        start: Option<NodeId>,
        end: Option<NodeId>,
        exclusive: bool,
    },
    /// `P1 | P2 | ...` -- matches if ANY alternative matches. Real Ruby
    /// forbids any alternative from binding a variable (there's no single
    /// consistent binding to expose otherwise) -- enforced at LOWERING time
    /// (a clean rejection, not silently dropped bindings; see
    /// `parse/mod.rs::lower_pattern`'s validation), so by the time this
    /// reaches `codegen` every nested `Pattern` here is guaranteed
    /// binding-free.
    Or(Vec<Pattern>),
    /// `PAT => name` -- binds `name` to the scrutinee ONLY IF `PAT` itself
    /// matches (unlike a bare `Bind`, which always matches). The common,
    /// load-bearing shape is `Capture(Box::new(ClassCheck(_)), name)`
    /// (`in Integer => n`), which `codegen::patterns::collect_narrowing`
    /// recognizes specially to statically narrow `name`'s inferred type for
    /// the rest of the matching arm's body (see that function's docs for
    /// the deliberate, safety-motivated scope-cut: only built-in, non-
    /// `Object` types are narrowed this way).
    Capture(Box<Pattern>, String),
    /// `[pre.., *rest, post..]` (a `constant` guard, e.g. `Point[x, y]`, is
    /// optional). `rest`: `None` = no `*` at all (exact-length match);
    /// `Some(None)` = an anonymous `*` (discards the middle slice);
    /// `Some(Some(name))` = `*name` binds the middle slice as a new Array.
    Array {
        constant: Option<String>,
        pre: Vec<Pattern>,
        rest: Option<Option<String>>,
        post: Vec<Pattern>,
    },
    /// `[*, mid.., *]` -- a Find pattern: `mid` must match SOME contiguous
    /// window of the scrutinee array (searched left to right, first match
    /// wins), unlike `Array`'s fixed pre/post anchoring. Real Ruby's grammar
    /// requires a splat on BOTH sides (that's the defining shape of a Find
    /// pattern), so `pre_rest`/`post_rest` are always present as a splat,
    /// only their NAME is optional (`None` = anonymous, discards that side's
    /// leftover slice).
    Find {
        constant: Option<String>,
        pre_rest: Option<String>,
        mid: Vec<Pattern>,
        post_rest: Option<String>,
    },
    /// `{key: pattern, ..., **rest}` (a `constant` guard is optional, same
    /// as `Array`). Each pair's value is `None` for the shorthand `{key:}`
    /// form (binds a local named `key` directly -- real Ruby sugar, not a
    /// distinct pattern shape), `Some(pattern)` otherwise. `rest` -- see
    /// `HashPatternRest`'s docs.
    Hash {
        constant: Option<String>,
        pairs: Vec<(String, Option<Pattern>)>,
        rest: HashPatternRest,
    },
}

/// The three shapes a hash pattern's `**` tail can take -- distinct from the
/// `Option<Option<String>>` shape `Array`/`Find`'s splats use because `**nil`
/// (explicit "no other keys allowed") has no positional-splat equivalent.
#[derive(Clone)]
pub enum HashPatternRest {
    /// No `**` at all -- extra keys in the scrutinee are simply ignored
    /// (real Ruby's default hash-pattern leniency).
    None,
    /// `**rest` / anonymous `**` -- binds the leftover key/value pairs (not
    /// matched by any explicit `pairs` entry) as a new Hash when named.
    Rest(Option<String>),
    /// `**nil` -- the scrutinee must have EXACTLY the declared keys, no more.
    NoMoreKeys,
}

impl Pattern {
    /// Every `NodeId` directly embedded in this pattern (a pinned/value
    /// expression, a range endpoint) -- NOT the pattern's own guard/arm body
    /// (those live on `PatternArm`, a sibling concern). Shared by every
    /// exhaustive-match site that needs to walk sub-expressions nested
    /// inside a pattern (ivar collection, bare-`yield`/`block_given?`
    /// scanning, local-type tracking, capture analysis) so that traversal
    /// logic lives in exactly one place.
    pub fn for_each_node(&self, visit: &mut impl FnMut(NodeId)) {
        match self {
            Pattern::Bind(_) | Pattern::ClassCheck(_) => {}
            Pattern::Value(n) | Pattern::Pin(n) => visit(*n),
            Pattern::Range { start, end, .. } => {
                if let Some(n) = start {
                    visit(*n);
                }
                if let Some(n) = end {
                    visit(*n);
                }
            }
            Pattern::Or(pats) => {
                for p in pats {
                    p.for_each_node(visit);
                }
            }
            Pattern::Capture(inner, _) => inner.for_each_node(visit),
            Pattern::Array { pre, post, .. } => {
                for p in pre.iter().chain(post) {
                    p.for_each_node(visit);
                }
            }
            Pattern::Find { mid, .. } => {
                for p in mid {
                    p.for_each_node(visit);
                }
            }
            Pattern::Hash { pairs, .. } => {
                for (_, p) in pairs {
                    if let Some(p) = p {
                        p.for_each_node(visit);
                    }
                }
            }
        }
    }

    /// Every local-variable name this pattern binds if it matches -- these
    /// leak into the enclosing METHOD scope exactly like an `if`/`case`
    /// branch's locals do (no new Ruby scope), so `codegen::hoisting`'s
    /// whole-scope local collection needs to see every one of them up
    /// front, same as `HirNode::MultiWrite`'s targets. Shared by that
    /// collection pass and `codegen::patterns::collect_narrowing`.
    pub fn for_each_bound_name(&self, visit: &mut impl FnMut(&str)) {
        match self {
            Pattern::Bind(name) => visit(name),
            Pattern::Value(_)
            | Pattern::Pin(_)
            | Pattern::ClassCheck(_)
            | Pattern::Range { .. } => {}
            Pattern::Or(pats) => {
                // No-op in practice -- lowering rejects any binding pattern
                // inside `|` -- but walking is harmless and keeps this
                // function a total, structural traversal rather than
                // silently assuming the invariant holds.
                for p in pats {
                    p.for_each_bound_name(visit);
                }
            }
            Pattern::Capture(inner, name) => {
                inner.for_each_bound_name(visit);
                visit(name);
            }
            Pattern::Array {
                pre, rest, post, ..
            } => {
                for p in pre.iter().chain(post) {
                    p.for_each_bound_name(visit);
                }
                if let Some(Some(name)) = rest {
                    visit(name);
                }
            }
            Pattern::Find {
                pre_rest,
                mid,
                post_rest,
                ..
            } => {
                if let Some(name) = pre_rest {
                    visit(name);
                }
                for p in mid {
                    p.for_each_bound_name(visit);
                }
                if let Some(name) = post_rest {
                    visit(name);
                }
            }
            Pattern::Hash { pairs, rest, .. } => {
                for (key, p) in pairs {
                    match p {
                        Some(p) => p.for_each_bound_name(visit),
                        // The `{key:}` shorthand binds a local named `key`
                        // directly -- see `Pattern::Hash`'s docs.
                        None => visit(key),
                    }
                }
                if let HashPatternRest::Rest(Some(name)) = rest {
                    visit(name);
                }
            }
        }
    }
}

/// One `in PATTERN [if/unless GUARD]` arm of a `case/in`.
#[derive(Clone)]
pub struct PatternArm {
    pub pattern: Pattern,
    /// `(condition, is_unless)` -- `unless` negates the same way `HirNode::While`'s
    /// `negate` flag does, rather than being a separate boolean-inverted
    /// node kind.
    pub guard: Option<(NodeId, bool)>,
    pub body: Vec<NodeId>,
}

/// A single multi-assignment target slot (`a, b = ...`'s `a`/`b`, or a
/// nested `(a, b), c = ...`'s `(a, b)`) -- generalizes the original
/// plain-local-only shape to every real Ruby assignment target kind, mirrored
/// directly from `MultiWriteNode`/`MultiTargetNode`'s own recursive
/// `lefts`/`rest`/`rights` grammar (see `MultiTargetGroup`). `Call` covers
/// BOTH `obj.attr = ...` and `arr[i] = ...` targets uniformly: both are
/// ordinary Ruby method calls (`attr=`/`[]=`), so lowering PRE-BUILDS the
/// actual write `Call` node (`write_call`) with a synthetic hidden local
/// (`tmp_name`) standing in for "the value this target will receive" as its
/// final argument -- codegen only has to bind `tmp_name` to the runtime-
/// destructured value BEFORE emitting `write_call` via the ordinary
/// `emit_expr` path, reusing the EXACT same static/dynamic dispatch
/// `codegen::call::dispatch` already provides for every other call, with no
/// bespoke attr/index-write codegen of its own. See `parse::lower_multi_target`.
// `Clone` because `Params` is `Clone` and now carries destructuring groups
// (`Params::destructures`); the targets themselves are small, owned data.
#[derive(Clone)]
pub enum MultiTarget {
    Local(String),
    Ivar(String),
    ClassVar(String),
    Global(String),
    /// A bare (lexically-scoped) constant target -- see `ConstWrite`'s docs
    /// for the same `scope: None` resolution rule.
    Const(String),
    /// An explicit `Foo::BAR` target -- `scope` resolves like
    /// `ConstWrite`'s `Some(class_name)`.
    ScopedConst {
        scope: String,
        name: String,
    },
    /// `obj.attr = tmp_name` / `arr[i] = tmp_name` -- see this enum's own
    /// docs above.
    Call {
        write_call: NodeId,
        tmp_name: String,
    },
    /// `(a, b)` -- a nested destructuring group; the value distributed to
    /// this slot is itself further split via `zeo_rt::multi_assign`,
    /// recursively.
    Nested(MultiTargetGroup),
}

/// The `before`/`splat`/`after` shape EVERY multi-assignment target list
/// has, at every nesting level (Ruby's own grammar allows at most one splat
/// per group, anchoring plain targets before/after it) -- shared by the
/// top-level `HirNode::MultiWrite`, a `MultiTarget::Nested` group, and a
/// multi-target `for a, b in ...`'s own index (`HirNode::For`). `splat:
/// None` = no `*` at all; `Some(None)` = an anonymous `*` (discards the
/// middle slice -- still unsupported, matching the pre-existing plain-local
/// restriction, a clean lowering error); `Some(Some(target))` = `*target`.
#[derive(Clone)]
pub struct MultiTargetGroup {
    pub before: Vec<MultiTarget>,
    pub splat: Option<Option<Box<MultiTarget>>>,
    pub after: Vec<MultiTarget>,
}

impl MultiTargetGroup {
    /// Every plain LOCAL name this group binds, at any nesting depth,
    /// appended to `out`. Only locals: an ivar/global/constant/attr-write
    /// target isn't a name the enclosing scope binds, so it is irrelevant to
    /// the callers (`Params::bound_names`, which answers "what does this
    /// parameter list introduce as locals?").
    pub fn collect_local_names(&self, out: &mut Vec<String>) {
        let mut visit = |t: &MultiTarget| match t {
            MultiTarget::Local(n) => out.push(n.clone()),
            MultiTarget::Nested(g) => g.collect_local_names(out),
            _ => {}
        };
        self.before.iter().for_each(&mut visit);
        if let Some(Some(t)) = &self.splat {
            visit(t);
        }
        self.after.iter().for_each(&mut visit);
    }
}

impl MultiTarget {
    /// Every `NodeId` directly embedded in this target (a `Call` target's
    /// pre-built `write_call`, or a nested group's own embedded nodes) --
    /// NOT the multi-assignment's own `value` (a sibling concern) -- mirrors
    /// `Pattern::for_each_node`'s exact shape/purpose, shared by every
    /// traversal that needs to walk sub-expressions nested inside a target
    /// (ivar/cvar collection, local-type tracking, hoisting, capture
    /// analysis).
    pub fn for_each_node(&self, visit: &mut impl FnMut(NodeId)) {
        match self {
            MultiTarget::Local(_)
            | MultiTarget::Ivar(_)
            | MultiTarget::ClassVar(_)
            | MultiTarget::Global(_)
            | MultiTarget::Const(_)
            | MultiTarget::ScopedConst { .. } => {}
            MultiTarget::Call { write_call, .. } => visit(*write_call),
            MultiTarget::Nested(group) => group.for_each_node(visit),
        }
    }

    /// Every LEAF target this one denotes, at any nesting depth. A `Nested`
    /// group recurses; everything else yields itself.
    ///
    /// The companion to `for_each_node`, which yields the sub-EXPRESSIONS
    /// embedded in a target. This yields the targets themselves, so a pass
    /// collecting names by storage class -- the ivar/cvar/const registration
    /// that decides a struct's fields and a constant's owning scope -- has
    /// something to match on.
    ///
    /// `for_each_node` silently dropped every ivar, cvar and constant
    /// written only via a multi-assignment or a `for` target, because those
    /// arms are leaves with no embedded expression. An ivar with no
    /// collected name gets no struct field, and its write does not compile.
    /// A cvar or constant registers against the wrong owner, which is worse:
    /// it compiles.
    ///
    /// `Global` targets need nothing from this. `emit_target_write` lowers
    /// them through `zeo_rt::global_set`, which declares no storage.
    pub fn for_each_target(&self, visit: &mut impl FnMut(&MultiTarget)) {
        match self {
            MultiTarget::Nested(group) => group.for_each_target(visit),
            leaf => visit(leaf),
        }
    }

    /// Every LOCAL-like name this target binds, for hoisting/local-type-
    /// tracking purposes: a plain `Local`, or a `Call` target's own hidden
    /// `tmp_name` synthetic local (bound once per multi-assignment, read
    /// back inside `write_call` -- see `MultiTarget::Call`'s docs). Ivar/
    /// cvar/global/const targets have their own, separate storage and don't
    /// participate in the enclosing scope's LOCAL-variable bookkeeping at
    /// all, so they're excluded here (mirrors `Pattern::for_each_bound_name`'s
    /// same "leaks into the enclosing scope" contract, narrowed to only the
    /// target kinds that actually use local-variable storage).
    pub fn for_each_local_name(&self, visit: &mut impl FnMut(&str)) {
        match self {
            MultiTarget::Local(n) => visit(n),
            MultiTarget::Call { tmp_name, .. } => visit(tmp_name),
            MultiTarget::Ivar(_)
            | MultiTarget::ClassVar(_)
            | MultiTarget::Global(_)
            | MultiTarget::Const(_)
            | MultiTarget::ScopedConst { .. } => {}
            MultiTarget::Nested(group) => group.for_each_local_name(visit),
        }
    }
}

impl MultiTargetGroup {
    /// See `MultiTarget::for_each_node`'s docs.
    pub fn for_each_node(&self, visit: &mut impl FnMut(NodeId)) {
        for t in self.before.iter().chain(&self.after) {
            t.for_each_node(visit);
        }
        if let Some(Some(t)) = &self.splat {
            t.for_each_node(visit);
        }
    }

    /// See `MultiTarget::for_each_target`'s docs.
    pub fn for_each_target(&self, visit: &mut impl FnMut(&MultiTarget)) {
        for t in self.before.iter().chain(&self.after) {
            t.for_each_target(visit);
        }
        if let Some(Some(t)) = &self.splat {
            t.for_each_target(visit);
        }
    }

    /// See `MultiTarget::for_each_local_name`'s docs.
    pub fn for_each_local_name(&self, visit: &mut impl FnMut(&str)) {
        for t in self.before.iter().chain(&self.after) {
            t.for_each_local_name(visit);
        }
        if let Some(Some(t)) = &self.splat {
            t.for_each_local_name(visit);
        }
    }
}

/// One `rescue [classes] [=> binding] ... end` clause of a `begin`/an
/// implicit method-body rescue. `classes` empty = a bare `rescue` -- matches
/// `StandardError` and its descendants (real Ruby's own default), NOT
/// literally every `Exception` -- see `codegen::exceptions`'s docs for the
/// matching codegen. `binding`'s name leaks into the enclosing METHOD scope
/// exactly like a `case/in` pattern's bound names do (no new Ruby scope) --
/// `codegen::hoisting`'s whole-scope local collection needs to see it up
/// front, same treatment as `Pattern::for_each_bound_name`'s callers.
#[derive(Clone)]
pub struct RescueClause {
    /// The clause's STATIC exception classes -- plain constant references
    /// (`rescue Foo, Bar => e`), resolved to `ClassId`s and matched with a
    /// compile-time `is_a` ancestry check.
    pub classes: Vec<String>,
    /// Splatted exception lists (`rescue *errs => e`): each is a runtime
    /// expression evaluating to an Array of exception classes (or a single
    /// class). Matched at runtime via `zeo_rt::rescue_matches_any`, OR'd in
    /// after `classes`. NOTE: a side-effecting splat expr placed BEFORE a
    /// matching literal class evaluates slightly out of CRuby's strict
    /// left-to-right short-circuit order -- a documented, negligible divergence
    /// (the boolean MATCH result is always identical).
    pub splats: Vec<NodeId>,
    pub binding: Option<String>,
    pub body: Vec<NodeId>,
}

/// One part of a (possibly-interpolated) string literal. A plain `"..."`
/// with no `#{}` lowers to a single `Lit` part. Only a single bare expression
/// is supported inside `#{}` (mirrors `ParenthesesNode`'s single-statement
/// restriction) -- a multi-statement interpolation body is a clean lowering
/// error, not silently truncated to its last statement.
#[derive(Clone)]
pub enum StrPart {
    Lit(String),
    /// A literal segment whose bytes are NOT valid UTF-8 -- a `"\xNN"`
    /// escape that doesn't form a character (Ruby tags such a literal
    /// ASCII-8BIT). Kept as raw bytes so the encoding engine sees them
    /// verbatim instead of the `String::from_utf8_lossy` U+FFFD mangling.
    Bytes(Vec<u8>),
    Interp(NodeId),
}

/// A `/pattern/flags` / `%r{pattern}flags` literal's option letters --
/// `ruby-prism`'s `RegularExpressionNode`/`InterpolatedRegularExpressionNode`
/// expose several more (`o`/`e`/`n`/`s`/`u`, all encoding/interpolation-once
/// concerns), but zeo is UTF-8-only throughout (see
/// `docs/limitations.md`'s existing posture on strings), so only the three
/// letters that change actual MATCHING semantics are modeled; the rest are
/// silently accepted as no-ops except a genuinely non-UTF-8-forcing encoding
/// flag (`e`/`s`), which is a clean lowering rejection (see
/// `parse/mod.rs`'s recognizer).
#[derive(Clone, Copy, Default)]
pub struct RegexpFlags {
    /// `i` -- case-insensitive matching.
    pub ignore_case: bool,
    /// `x` -- ignore unescaped whitespace and `#` comments in the pattern
    /// source (`regex`'s `ignore_whitespace`).
    pub extended: bool,
    /// `m` -- real Ruby's `/m` makes `.` match a newline too (`regex`'s
    /// `dot_matches_new_line`) -- NOT the same thing as most other regex
    /// flavors' "multi-line mode" (`regex`'s own `multi_line`, which
    /// governs `^`/`$`). Ruby's `^`/`$` ALWAYS match at line boundaries by
    /// default, with no opt-in flag at all -- see
    /// `zeo_rt::regexp::regexp_new`'s docs for how this is modeled
    /// (`multi_line(true)` is unconditional, independent of this field).
    pub multiline: bool,
    /// `n`/`e`/`s`/`u` -- the encoding the literal FORCES. Not decoration:
    /// `#options`, `#encoding` and `#fixed_encoding?` each report it. See
    /// [`zeo_abi::RegexpEncoding`].
    pub encoding: zeo_abi::RegexpEncoding,
}

/// A C ABI type for one FFI argument or return value (the real `ffi`
/// gem's type keywords): the scalars, `:pointer`/`:string`, named `enum`s, and
/// `callback` function-pointer types -- enough for a faithful `attach_function`
/// over libc/libm and most C entry points. Each maps to a C type codegen
/// declares in the `extern "C"` block (or, for variadic calls and callbacks,
/// to the libffi marshaling it emits) and to the RubyValue↔C conversion.
/// `Clone`, not `Copy`: the `Enum` and `Callback` variants carry owned data
/// (a member table / a nested signature), resolved at parse time and embedded
/// so codegen needs no runtime type registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FfiType {
    Void,
    /// `:char`/`:int8`, `:short`/`:int16`, `:int`/`:int32`, `:long`/`:int64`.
    Int(u8), // width in BITS: 8/16/32/64
    /// `:uchar`/`:uint8` … `:ulong`/`:uint64`, and `:size_t` (→ 64 here, LP64).
    Uint(u8),
    /// `:float` (32) / `:double` (64).
    Float(u8),
    /// `:bool` -- a C `bool`/`_Bool`.
    Bool,
    /// `:string` -- a C `const char *`. As an argument: a NUL-terminated copy
    /// of the Ruby String, valid for the call. As a return: read the C string
    /// back into a Ruby String (`nil` for a NULL pointer).
    Str,
    /// `:pointer` -- an opaque `void *`. As an argument: the raw address an
    /// `FFI::Pointer`/`MemoryPointer` (or `nil` = NULL) carries. As a return:
    /// wrap the address as an `FFI::Pointer`. See `ext::ffi`.
    Pointer,
    /// A named `enum :tag, [:sym, val, ...]`. The underlying C type is `int`.
    /// As an argument: a Symbol maps to its int (an Integer passes through). As
    /// a return: a mapped int becomes its Symbol (an unmapped int stays an
    /// Integer), exactly as the gem's `Enum` data-converter does. The `Vec`
    /// holds `(symbol_name, value)` members in declaration order.
    Enum(Vec<(String, i64)>),
    /// An `enum` whose MEMBERS only a running process can produce: ethon's
    /// `enum(:easy_code, easy_codes)` calls a method on a module it
    /// `extend`ed, gir_ffi builds its flag sets off type information read out
    /// of a shared library. The ABI never depended on the members -- an enum
    /// is an `int` either way -- so the signature is decided here and the
    /// marshaling table waits for the class body to run. The `usize` ties this
    /// type to the statement's runtime store
    /// (`zeo_rt::ffi::enum_store`), exactly as [`FfiLib::Deferred`] does for a
    /// library. See `Hir::ffi_enum_slots`.
    EnumSlot(usize),
    /// A `callback :tag, [arg_types], ret_type` -- a C function-pointer type. As
    /// an argument, a Ruby Proc is marshaled into a libffi closure trampolining
    /// into the Proc, and the closure's code pointer is passed. Holds the
    /// callback's `(arg_types, return_type)` so the closure's own CIF can be
    /// built. (As a C ABI type it is a `void *`.)
    Callback(Vec<FfiType>, Box<FfiType>),
    /// An INLINE array field of a struct layout -- `layout :bytes, [:uint8, 4]`.
    /// It occupies `count` elements in place (it is not a pointer), so it is
    /// what decides every following field's offset. Reading the field yields
    /// ruby's own proxy over the same memory rather than a copy: an
    /// `FFI::Struct::InlineArray`, or an `FFI::StructLayout::CharArray` when
    /// the element is 8-bit, which is the one that also answers `to_s`.
    /// Only legal in a layout; the gem has no inline array in a function
    /// signature either.
    Array(Box<FfiType>, usize),
    /// A struct passed or stored BY VALUE -- `.by_value` in a signature, or
    /// a bare struct name in a LAYOUT field (an inline nested struct).
    /// Carries the full layout: the ABI needs every field's real type, not
    /// just the byte count (a `{float, float}` classifies differently from
    /// `[u8; 8]` on SysV/AArch64).
    Struct(FfiStructLayout),
    /// A BARE `FFI::Struct` subclass name in a SIGNATURE position --
    /// ruby-ffi's `StructByReference`, whose ABI type is a POINTER
    /// (`find_type` wraps a struct class this way; `.by_value` is the
    /// by-value spelling). An argument takes a struct instance; a RETURN
    /// comes back as a plain `FFI::Pointer`, never auto-wrapped in the
    /// class (oracle-verified). Lowering degrades it to `Pointer` at each
    /// boundary, so codegen never sees it; the class path rides along for
    /// the one consumer that rejects it (a callback signature).
    StructRef(String),
    /// A POSIX integer typedef whose width GENUINELY differs between the
    /// targets zeo builds for (`mode_t` is u16 on macOS, u32 on glibc; also
    /// `dev_t`, `suseconds_t`, `clock_t`, ...). Legal in argument/return
    /// position only, where generated code spells the TARGET's own
    /// `zeo_rt::libc::<name>` and rustc (or a runtime `size_of`) supplies
    /// the real width at build time. NOT legal in a struct layout: a field
    /// width that shifts by target would silently shift every later offset
    /// -- that stays `CScalar::from_c_typedef`'s documented rejection.
    PlatformScalar(String),
    /// `:strptr` -- a `char *` RETURN read back as the gem's two-element
    /// `[String, Pointer]` (the decoded string AND the raw pointer, so the
    /// caller can still free it). Return position only.
    StrPtr,
}

impl From<zeo_abi::ffi::CScalar> for FfiType {
    fn from(s: zeo_abi::ffi::CScalar) -> FfiType {
        use zeo_abi::ffi::CScalar as S;
        match s {
            S::Void => FfiType::Void,
            S::I8 => FfiType::Int(8),
            S::I16 => FfiType::Int(16),
            S::I32 => FfiType::Int(32),
            S::I64 => FfiType::Int(64),
            S::U8 => FfiType::Uint(8),
            S::U16 => FfiType::Uint(16),
            S::U32 => FfiType::Uint(32),
            S::U64 => FfiType::Uint(64),
            S::F32 => FfiType::Float(32),
            S::F64 => FfiType::Float(64),
            S::Bool => FfiType::Bool,
            S::Str => FfiType::Str,
            S::Pointer => FfiType::Pointer,
        }
    }
}

impl FfiType {
    /// The C ABI scalar this type marshals AS: an enum is a C `int`, a
    /// callback passes as its code pointer. `None` for the two types with no
    /// scalar representation at a call site (an inline array lives only in a
    /// struct layout; a by-value struct is its own aggregate).
    pub fn c_scalar(&self) -> Option<zeo_abi::ffi::CScalar> {
        use zeo_abi::ffi::CScalar as S;
        Some(match self {
            FfiType::Void => S::Void,
            FfiType::Int(w) => match w {
                8 => S::I8,
                16 => S::I16,
                64 => S::I64,
                _ => S::I32,
            },
            FfiType::Uint(w) => match w {
                8 => S::U8,
                16 => S::U16,
                64 => S::U64,
                _ => S::U32,
            },
            FfiType::Float(64) => S::F64,
            FfiType::Float(_) => S::F32,
            FfiType::Bool => S::Bool,
            FfiType::Str => S::Str,
            FfiType::Pointer | FfiType::Callback(..) | FfiType::StructRef(_) => S::Pointer,
            FfiType::Enum(_) | FfiType::EnumSlot(_) => S::I32,
            FfiType::Array(..)
            | FfiType::Struct(_)
            | FfiType::PlatformScalar(_)
            | FfiType::StrPtr => return None,
        })
    }
}

/// An `FFI::Struct` subclass's computed C layout, recorded when its `layout`
/// directive lowers so later declarations can pass it by value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfiStructLayout {
    /// The class name as written -- for diagnostics and a future by-value
    /// RETURN wrap (which must construct this class).
    pub class_path: String,
    /// `(field, type, byte offset)` in declaration order.
    pub fields: Vec<(String, FfiType, usize)>,
    pub size: usize,
    pub align: usize,
    pub union: bool,
}

/// Where an `attach_function`'s symbol comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FfiLib {
    /// A name rustc can link at build time: `#[link(name = ..)]` plus a
    /// direct `extern "C"` call -- the zero-overhead tier.
    Static(String),
    /// Library candidates only a RUNNING process can open: a bundled `.so`
    /// path built from `__dir__`, a versioned soname, a candidate list. The
    /// call site `dlopen`s the first that opens and `dlsym`s once -- which is
    /// when and how CRuby's ffi gem binds every symbol. See
    /// `zeo_rt::ffi::FfiSymSite`.
    Runtime(Vec<String>),
    /// An `ffi_lib` whose candidates NO compile-time fold can name (an ENV
    /// read, a local, a helper call like `FFI.map_library_name`): the
    /// expressions evaluate when the class body EXECUTES, dlopen eagerly
    /// there (CRuby's require-time `LoadError`, to the statement), and the
    /// handle lands in this slot for every `attach_function` under it. The
    /// call site resolves through the same dlopen/libffi tier as `Runtime`.
    Deferred { slot: usize },
    /// No `ffi_lib` (or `FFI::CURRENT_PROCESS`): the always-linked image.
    None,
}

/// One C function a module `attach_function`'d. The synthesized wrapper
/// method's whole body IS this node -- see `codegen`'s `emit_ffi_call`, which
/// declares the `extern "C"` symbol fn-locally (with `#[link(name = ..)]`, so no
/// build-step change is needed), marshals each argument, calls it, and wraps the
/// result back into a `RubyValue`.
#[derive(Clone)]
pub struct FfiCall {
    /// The C symbol to declare and call (the `attach_function` C name, which may
    /// differ from the Ruby method name in the 4-arg rename form).
    pub symbol: String,
    /// The library the symbol lives in.
    pub lib: FfiLib,
    /// Each FIXED argument: the wrapper param to read (`LocalRead`) and its C
    /// type. For a variadic function these are only the declared leading
    /// arguments (the ones before `:varargs`).
    pub args: Vec<(NodeId, FfiType)>,
    /// The C return type -- governs the wrap back to a `RubyValue`.
    pub ret: FfiType,
    /// `Some(rest)` for a variadic function (`attach_function [.., :varargs]`),
    /// where `rest` reads the wrapper's `*rest` param -- a flat Array of
    /// alternating `type_symbol, value` pairs marshaled at runtime via libffi.
    /// `None` for an ordinary fixed-arity call.
    pub variadic: Option<NodeId>,
    /// `attach_function ..., blocking: true` -- the call runs with the GVL
    /// released, so other ruby threads keep running across a long C call. Under
    /// zeo's default parallel mode there is no GVL to release and
    /// `zeo_rt::without_gvl` is a no-op; under `ZEO_GVL=1` it is the handoff
    /// the option asks for.
    pub blocking: bool,
}

/// A real enum of node kinds; growing it is additive (new variants).
#[derive(Clone)]
pub enum HirNode {
    Program(Vec<NodeId>),
    IntegerLit(i64),
    /// An Integer literal beyond i64 (the bignum representation) -- carried as
    /// prism's own `(negative, LSB-first u32 digits)` shape so zeo
    /// needs no bigint dependency; codegen emits
    /// `zeo_rt::int_from_u32_digits`. Types as `Int` like `IntegerLit`
    /// (one Ruby Integer class, two payloads).
    BigIntegerLit {
        negative: bool,
        digits: Vec<u32>,
    },
    /// `3r` / `1.5r` -- prism pre-rationalizes the decimal
    /// forms (`1.5r` arrives as numerator 3, denominator 2), so both
    /// components travel as digit strings like `BigIntegerLit`. The
    /// denominator is positive and non-zero by syntax.
    RationalLit {
        negative: bool,
        num_digits: Vec<u32>,
        den_digits: Vec<u32>,
    },
    /// `4i` / `2.0i` / `3ri` -- an imaginary literal wrapping
    /// its lowered inner numeric literal compositionally
    /// (`Complex(0, inner)`).
    ImaginaryLit(NodeId),
    /// A real `f64` payload -- `HirNode` itself derives no `Eq`/`Hash` (see
    /// this enum's own docs), so an un-`Eq`-able float here is no different
    /// from `IntegerLit`'s `i64` in that respect.
    FloatLit(f64),
    SymbolLit(String),
    /// `nil` as a literal. `case/in`'s `Pattern::Value` fallback needs
    /// `in nil` to lower through the ordinary expression path like any
    /// other literal, rather than special-casing pattern lowering around it.
    NilLit,
    /// `true` / `false` -- see `NilLit`'s docs; same reasoning.
    BoolLit(bool),
    /// `a && b` / `a and b` -- prism normalizes both spellings to the same
    /// node (only precedence differs, already resolved by parse time).
    /// Short-circuits like Ruby's real `&&`, not like Rust's bool-typed
    /// `&&`: the *operand itself* is returned (`a` if falsy, else `b`), so
    /// codegen can't emit a literal Rust `&&` here (see `expr.rs`).
    And(NodeId, NodeId),
    /// `a || b` / `a or b` -- see `And`'s docs; same short-circuit-the-
    /// operand-not-a-bool semantics.
    Or(NodeId, NodeId),
    /// `defined?(expr)` -- a compile-time-resolvable classification of
    /// `expr`'s syntactic form (mirrors CRuby's `"expression"`/`"method"`/
    /// `"local-variable"`/`"instance-variable"`/`nil` results), not a
    /// runtime check. See `codegen::expr::emit_defined`'s docs for the
    /// scope-cut this approximates.
    Defined(NodeId),
    /// `if`/`unless`/`elsif`/ternary all normalize to this at lowering time
    /// (`unless` swaps `then_body`/`else_body`; `elsif` is prism's own
    /// `IfNode::subsequent()` recursion, which lowering walks into a nested
    /// `If`; ternary is literally the same `IfNode` shape prism produces for
    /// `a ? b : c`). Ruby's implicit-last-expression-return and Rust's
    /// `if`-as-expression are structurally identical, so this maps directly
    /// onto a Rust `if/else` expression in tail position (see `codegen::expr`).
    If {
        cond: NodeId,
        then_body: Vec<NodeId>,
        else_body: Vec<NodeId>,
    },
    /// `case subject; when v1, v2 then ...; else ...; end` -- value
    /// matching only (no subject means each `when` value is itself the
    /// boolean condition, like a chained `if`/`elsif`). `case/in` pattern
    /// matching is a distinct prism node (`CaseMatchNode`) and isn't
    /// lowered to this.
    CaseWhen {
        subject: Option<NodeId>,
        /// Each arm's `(conditions, body)`. The conditions are
        /// `ArrayElem`s, the same shape a call's arguments use, so
        /// `when *candidates` needs no machinery of its own -- a `Splat`
        /// element tests EVERY element of its array, which is exactly what
        /// the `||` chain over the non-splat ones already does, just with
        /// the arity known only at runtime.
        arms: Vec<(Vec<ArrayElem>, Vec<NodeId>)>,
        else_body: Vec<NodeId>,
    },
    /// `[1, 2, *rest]` -- see `ArrayElem`'s docs for the splat handling.
    ArrayLit(Vec<ArrayElem>),
    /// `{ a: 1, b: 2, **other }` -- an ordered list of pairs and double-splats
    /// (see `KwArg`). Merged left-to-right, last key wins.
    HashLit(Vec<KwArg>),
    /// `a..b` / `a...b` -- either endpoint may be absent (`a..`/`..b`),
    /// matching Ruby's beginless/endless ranges.
    RangeLit {
        start: Option<NodeId>,
        end: Option<NodeId>,
        exclusive: bool,
    },
    /// A (possibly-interpolated) string literal -- see `StrPart`'s docs.
    StringLit(Vec<StrPart>),
    /// `/pattern/flags` / `%r{pattern}flags`, possibly interpolated -- reuses
    /// `StrPart` wholesale (a regex literal's `#{}` interpolation is
    /// structurally identical to a string's, see `parse/mod.rs`'s recognizer,
    /// which shares `lower_string_part`). A regex compile failure (either a
    /// static, unconditionally-invalid pattern, or a runtime-only failure
    /// once an interpolated part is substituted in) raises a real, catchable
    /// `RegexpError` at codegen's construction site -- NOT rejected any
    /// earlier at `zeo` compile time, unlike real Ruby's own parse-time
    /// `SyntaxError` for a static pattern: a documented, narrower-timing
    /// approximation (see `codegen::collections::emit_regexp_lit`'s docs),
    /// not silent wrongness. Backed by the `regex` crate, not Ruby's own
    /// Onigmo engine -- no backreferences (`\1` inside the PATTERN itself,
    /// as opposed to a `gsub`/`sub` REPLACEMENT string, where they *are*
    /// supported -- see `zeo_rt::regexp`'s docs) and no lookaround
    /// (`(?=...)`/`(?!...)`/`(?<=...)`/`(?<!...)`), a real, documented
    /// semantic gap versus real Ruby, not an oversight.
    RegexpLit(Vec<StrPart>, RegexpFlags),
    LocalRead(String),
    LocalWrite(String, NodeId),
    IvarRead(String),
    IvarWrite(String, NodeId),
    /// `@@x` read/write. Ownership (which class/module's storage this
    /// actually refers to) is resolved once at `analyze` time by walking the
    /// referencing class's `ancestors` for an existing owner -- see
    /// `compiler::ClassInfo::cvar_owners`'s docs -- not re-resolved at
    /// codegen time, so this node just carries the bare name; codegen
    /// consults the already-resolved owner via `Ctx.current_class`.
    ClassVarRead(String),
    ClassVarWrite(String, NodeId),
    /// A bare constant used as a VALUE (currently only meaningful as a call
    /// receiver: `ClassName.foo`/`ModuleName.foo`) -- distinct from
    /// `ClassName.new(...)` (still its own `New` node) and from a
    /// superclass/include/extend/prepend target name (those stay plain
    /// `String`s, resolved directly, never wrapped in this). Resolves to
    /// "the class/module object named X" for dispatch purposes only; there
    /// is no first-class runtime `Class`/`Module` VALUE (can't be stored in
    /// a variable, compared, or reflected on at runtime).
    ClassRef(String),
    /// A literal-name-resolvable call: `recv.name(args) { block }`, or an
    /// implicit-self call (`receiver: None`).
    ///
    /// - `send`/`public_send` are ordinary calls named "send", not a separate
    ///   node kind. Codegen reads `name` and, for `send`, `args[0]`, to
    ///   choose static Path 1 or `zeo_rt::send` Path 2.
    /// - `safe: true` is `&.`, a flag on prism's same `CallNode`. The call
    ///   short-circuits to `nil` without evaluating when `receiver` is `nil`.
    /// - `args` reuses `ArrayElem`: a positional value, or a `*expr` splat
    ///   that flattens at runtime.
    /// - `kwargs` is the ordered `KwArg` list. Literal `name: value` pairs
    ///   interleave with `**h` double-splats in source order, and the merge
    ///   is left-to-right last-one-wins, so the order is observable. Any
    ///   `DoubleSplat` routes to the runtime arg-vector path; a pairs-only
    ///   list stays on Path 1. Codegen resolves the names against the
    ///   callee's declared keyword params, which a Path 1 site always knows.
    /// - `block_arg` is `foo(&existing_proc)`, forwarding an already-built
    ///   `Proc`. It is separate from `block`, a literal `{ }`/`do..end` at
    ///   the call site. Ruby rejects both on one call, and `ruby-prism`
    ///   rejects it at parse time, so zeo does not re-validate.
    Call {
        receiver: Option<NodeId>,
        name: String,
        args: Vec<ArrayElem>,
        kwargs: Vec<KwArg>,
        block: Option<NodeId>,
        block_arg: Option<NodeId>,
        safe: bool,
    },
    /// `ClassName.new(args)` -- a distinct node (not a plain `Call`) because
    /// it's always statically resolvable to a concrete class, and codegen
    /// needs that class name early to route ivar defaults / registration.
    ///
    /// `kwargs` carries a trailing `Foo.new(k: 1)`, kept SEPARATE from
    /// `args` the way `Call`'s own are, so `initialize`'s keyword
    /// parameters bind as keywords.
    ///
    /// `args` stays `Vec<NodeId>` rather than `Call`'s `Vec<ArrayElem>`: a
    /// SPLAT at a `.new` site (`Foo.new(*args)`) is still unsupported -- it
    /// needs the runtime arg-vector path, not this static one. See
    /// `parse`'s `.new` lowering. `kwargs` is the ordered `KwArg` list (pairs
    /// plus `**h` double-splats), the same as `Call`'s.
    New {
        class_name: String,
        args: Vec<NodeId>,
        kwargs: Vec<KwArg>,
        /// A literal block passed to `.new` (`Foo.new(x) { ... }`), forwarded
        /// to `initialize` so `yield`/`block_given?` inside it see the block.
        /// A block-PASS (`Foo.new(&p)`) still routes through the generic
        /// `Call` lowering instead (this stays `None`).
        block: Option<NodeId>,
    },
    /// A `super` call. Resolved at RUNTIME against the receiver's live
    /// linearized ancestry (`codegen::call::super_calls` picks the dispatch
    /// channel).
    ///
    /// `zsuper` distinguishes real Ruby's two zero-written-argument shapes,
    /// which mean OPPOSITE things: bare `super` (prism's
    /// `ForwardingSuperNode`, `zsuper: true`) forwards the current method's
    /// own parameters as currently bound, while `super()` (a `SuperNode`
    /// with no arguments, `zsuper: false`) passes NO arguments at all, so
    /// the parent's optionals take their defaults. `args` is always empty
    /// when `zsuper` is true.
    SuperCall {
        /// Explicit positional args (`super(a, *rest)`) as `ArrayElem`s, so a
        /// splat argument forwards through the runtime arg vector -- the same
        /// shape a `Call`'s args have. Always empty for bare `super`
        /// (`zsuper`), which forwards the current method's own params instead.
        args: Vec<ArrayElem>,
        /// Explicit keyword arguments (`super(x: 1, y: 2)`); bound to the
        /// parent's keyword params by NAME. Always empty for bare
        /// `super` (`zsuper`), which forwards the current method's own
        /// keywords instead.
        kwargs: Vec<KwArg>,
        zsuper: bool,
        /// A literal block written at the `super` site (`super { ... }` /
        /// `super(x) { ... }`) -- a `HirNode::Block`, bound as the spliced
        /// parent body's `__blk` so its `yield` runs this block. `None`
        /// means the parent implicitly sees the CURRENT method's own block
        /// (real Ruby forwards it), which the splice gets for free since
        /// `__blk` is already in scope there.
        block: Option<NodeId>,
        /// A block-PASS argument (`super(x, &blk)`) -- an expression coerced to
        /// a block via `to_proc` at the call. Mutually exclusive with `block`
        /// (Ruby's grammar forbids both), same either/or a `Call` carries.
        block_arg: Option<NodeId>,
    },
    Block {
        params: Box<Params>,
        body: Vec<NodeId>,
    },
    /// `-> (x) { ... }` / `lambda { ... }` -- a STANDALONE expression
    /// producing a real `RubyValue::Proc`, unlike `Block` (only ever reached
    /// via the `Call` that invokes it, see that variant's docs). Reuses
    /// `codegen::call::emit_proc_value`'s whole construction machinery
    /// (captures, redo-wrapper loop) via a shared helper, differing in
    /// exactly two ways real Ruby's own lambda semantics require: STRICT
    /// arity checking (raises `ArgumentError`, not a lenient nil-fill/drop),
    /// and `return`/`break` inside the body terminate the LAMBDA CALL
    /// itself (folded into a normal `Ok` return, like a method boundary)
    /// rather than propagating to the enclosing method/loop.
    Lambda {
        params: Box<Params>,
        body: Vec<NodeId>,
        /// True when this lambda is a METHOD BODY installed at runtime -- the
        /// desugar of a per-object singleton (`def obj.name`, `class << obj`)
        /// or a `def`/`define_method` in expression position. Such a body's
        /// `yield`/`block_given?`/`&blk` targets the block the METHOD is called
        /// with (threaded through `ProcData`'s call-site block slot via
        /// `with_self_and_block`), NOT the lexically enclosing method's block
        /// an ordinary lambda would clone in. CRuby draws the same line --
        /// `invoke_bmethod`'s specval vs the captured env (`vm.c:1786`).
        method_body: bool,
    },
    /// `class Name < Super ... end` / `module Name ... end` -- `is_module`
    /// distinguishes the two: a module has no `superclass` (always `None`)
    /// and is never instantiated (no `Name.new`, no generated Rust struct --
    /// see `codegen::mod::emit_class`'s docs). Both share this one node
    /// since everything about their BODY (methods, nested `include`/
    /// `extend`/`prepend`, class variables) lowers identically; only
    /// `analyze::register_class`'s registration differs.
    ClassDef {
        name: String,
        superclass: Option<String>,
        body: Vec<NodeId>,
        is_module: bool,
    },
    /// Also the desugared form of a literal-name `define_method(:name) { .. }`
    /// -- lowering (`parse/mod.rs`) treats it identically to a plain `def`,
    /// mirroring zeo's `walk_scope`. `is_class_method` is `def self.name`
    /// (`DefNode::receiver()` is `Some(SelfNode)`) -- a genuinely different
    /// registration target (`ClassInfo::class_methods`, not `::methods`; no
    /// `self: Arc<Self>` receiver at codegen time at all, see
    /// `compiler::ClassInfo`'s docs) even though the body shape is
    /// identical. Any OTHER explicit receiver: one that `names_enclosing_
    /// class` (net/smtp's `def SMTP.default_port`) is also a class method,
    /// and the rest desugar to `RECV.define_singleton_method` -- see
    /// `lower/mod.rs`'s def-receiver arms.
    DefMethod {
        name: String,
        params: Box<Params>,
        body: Vec<NodeId>,
        is_class_method: bool,
        visibility: Visibility,
        /// `true` for a real `def` keyword; `false` for a literal
        /// `define_method(:sym){...}` call desugared into this node. Only
        /// matters in EXPRESSION position (inside a block): a real `def`
        /// installs on the runtime default definee (a singleton method under an
        /// `instance_exec` on a plain object), whereas an explicit
        /// `define_method` is an ordinary `Module#define_method` call that
        /// raises `NoMethodError` when `self` isn't a Module/Class.
        is_def: bool,
    },
    /// `include Mod` -- one node per module argument, in left-to-right
    /// source order, when multiple are given (`include A, B` lowers to two
    /// sequential `Include` statements in the class body, matching real
    /// Ruby's "as if each were included one at a time, in order" rule). Only
    /// valid directly in a class/module body -- see
    /// `parse/mod.rs::lower_class_body_statement`.
    Include(String),
    /// `extend Mod` -- see `Include`'s docs; the module's OWN instance
    /// methods are materialized as CLASS methods on the extending class
    /// instead (`ClassInfo::class_methods`), not instance methods.
    Extend(String),
    /// `prepend Mod` -- see `Include`'s docs; the module is inserted BEFORE
    /// the class itself in the linearized `ancestors` list, so its methods
    /// take precedence over the class's own (reachable via `super`).
    Prepend(String),
    /// `prepend Mod` written inside `class << self` -- the singleton half of
    /// [`HirNode::Prepend`], exactly as [`HirNode::ClassMethodVisibility`] is
    /// the singleton half of `MethodVisibility`. The module's INSTANCE methods
    /// become the enclosing class's CLASS methods, ahead of its own `def
    /// self.x` (which they may override, `super` reaching the original).
    ///
    /// `ClassInfo::class_method_prepends` -- the same field the equivalent
    /// `C.singleton_class.prepend(M)` CALL form records (see
    /// `analyze::try_prepend_call_edit`), so `mro::materialize_class_methods`
    /// needs nothing new.
    ClassMethodPrepend(String),
    /// A definition REPORT: `Klass.method_added(:name)`, or one of its five
    /// siblings. Ruby announces every definition to the class it landed on, and
    /// the announcement runs at the definition's own position -- but zeo
    /// consumes a class-body `def` at analyze time and emits nothing there. So
    /// `analyze::def_hooks` splices one of these back in at that position, and
    /// only when a hook body will actually answer. Never produced by lowering:
    /// a program that defines no hook carries none of these at all.
    DefHook {
        /// The receiver of the report -- the class for `method_added`, and for
        /// `singleton_method_added` too, since a compiled singleton definition
        /// is always a class method (`def self.x`, `class << self`).
        class: u32,
        /// The hook's name, already resolved for the definition's shape.
        hook: String,
        /// The defined method's name, passed as the hook's one Symbol argument.
        name: String,
        /// Instance methods of `class` that are still in the FUTURE here.
        /// Ruby's hook sees a half-built class; zeo has every table installed
        /// before the first statement runs, so the reflection rows subtract
        /// this set for as long as the hook body is on the stack. Empty for
        /// the last definition in a class, which is the common case.
        pending: Vec<String>,
    },
    /// A method REDEFINITION applied at its document position. Ruby installs
    /// each `def` where it stands, so code running between two same-name
    /// `def`s dispatches to the FIRST body; zeo's static tables carry only
    /// the last-def-wins winner. `analyze::redefs` splices one of these at
    /// each superseded redefinition's position: codegen installs the named
    /// scope's compiled trampoline into the runtime overlay there, and a
    /// boot-time install of the FIRST body covers the window before it.
    /// Never produced by lowering.
    MethodRedefine {
        class: u32,
        name: String,
        /// The `ScopeId` (as raw index) whose body becomes current here.
        scope: u32,
    },
    /// `refine Target do ... end` in a module body. The block's `def`s lower
    /// into a HOLDER module (a `ClassDef` pushed immediately before this
    /// marker, named `#refinement:Target` so it claims no Ruby constant);
    /// this node names the class those methods refine. Deliberately not an
    /// `Include`: a refinement edits no ancestry at all -- it is consulted
    /// only at the call sites a `using` scope covers.
    ///
    /// `singleton` is `refine Target.singleton_class do ... end`: the holder's
    /// methods refine Target's CLASS methods (its singleton class), so a
    /// covered `Target.m` consults them where the plain form covers
    /// `instance.m`.
    Refine {
        target: String,
        holder: String,
        singleton: bool,
    },
    /// `using M` -- activates every refinement `M` holds for the code
    /// lexically AFTER this point, to the end of the enclosing body (the
    /// rest of the file at the top level). The position comes from this
    /// node's own span, so a `def` written after it is covered while one
    /// written above it is not, which is exactly real Ruby's rule.
    Using(String),
    /// `while cond ... end` / `until cond ... end` (+ modifier forms
    /// `stmt while cond` / `stmt until cond`) -- `until` folds in here as
    /// `negate: true`, exactly like `unless` folds into `If` by swapping
    /// branches (see `parse/mod.rs`). Always compiles to a labeled Rust
    /// `loop { }`, never a bare Rust `while` -- even for plain `while` --
    /// so `break value` has an expression-position target to jump to
    /// (Rust's own `while` is never an expression; see `codegen::loops`).
    /// The do-while form (`begin ... end while cond`, body always runs at
    /// least once) is a distinct prism shape wrapping a `BeginNode`, which
    /// isn't lowered -- it already falls through to the generic
    /// "unsupported syntax" error untouched, so it needs no explicit
    /// handling here.
    While {
        cond: NodeId,
        body: Vec<NodeId>,
        negate: bool,
        /// `begin ... end while cond` (prism's begin-modifier flag): a
        /// POST-test loop whose body runs once before the condition is first
        /// checked. `false` for the ordinary pre-test `while`/`until`.
        post: bool,
    },
    /// `loop do ... end` -- NOT a distinct `ruby-prism` node (it's an
    /// ordinary zero-arg, no-receiver `Kernel#loop` call with a block);
    /// `parse/mod.rs` desugars that call shape to this at lowering time,
    /// mirroring `define_method`'s existing call-shape desugar. An
    /// unconditional labeled Rust `loop { }` with no exit test of its own --
    /// only `break` (or an uncaught `raise`) ever ends it.
    Loop {
        body: Vec<NodeId>,
    },
    /// `for var in iterable ... end` / `for a, b in pairs ... end`. Unlike
    /// block-based iteration (`each { |x| ... }`), Ruby's `for` does NOT
    /// introduce a new variable scope: `var` and any locals first assigned
    /// in the body stay visible after the loop ends -- a real semantic
    /// difference, not a shortcut, and one `codegen`'s plain
    /// (non-block-nested) `let` emission already gives for free. `target`
    /// reuses `MultiTarget` (a plain `for x in ...` lowers to
    /// `MultiTarget::Local`; `for a, b in ...` to `MultiTarget::Nested`),
    /// destructured against each iterated element exactly like a
    /// `MultiWrite`'s value. The loop's own expression-position value is
    /// documented as `nil` unless a `break value` fires -- real Ruby returns
    /// the iterated collection itself in the no-break case, a
    /// narrower-than-real-Ruby simplification nothing in the corpus
    /// depends on.
    For {
        target: MultiTarget,
        iterable: NodeId,
        body: Vec<NodeId>,
    },
    /// `break` / `break value` -- unwinds to the end of the nearest *native*
    /// loop construct (`While`/`Loop`/`For`, or the pre-existing `.times`
    /// block-inlining special case in `codegen::call`), which `ruby-prism`
    /// itself already guarantees is the only place these can appear (a bare
    /// `break`/`next`/`redo` outside any loop/block is a parse error, not
    /// something lowering has to re-validate). Compiles to a literal Rust
    /// `break 'label value;` -- no `Signal` involved, per the ABI's stated
    /// scope-cut (see `signal.rs`), since a `break` inside a real escaping
    /// closure is handled separately. A multi-value `break a, b` lowers to a single
    /// implicit-array argument (`break [a, b]`), the same as `Return`/`Next`.
    Break(Option<NodeId>),
    /// `next` / `next value` -- ends the current iteration early, jumping to
    /// the loop's own re-test-the-condition point. See `Break`'s docs; the
    /// value is evaluated (for side effects) but otherwise discarded inside
    /// a native loop, matching real Ruby: `next value` only matters as "what
    /// the block call returns", which is meaningless for a bare `while`/
    /// `for`/`loop`.
    Next(Option<NodeId>),
    /// `redo` -- re-runs the current iteration's body from the top WITHOUT
    /// re-testing the loop condition or advancing (the one construct with no
    /// direct native Rust equivalent -- `continue` always re-tests/advances).
    /// See `codegen::loops`' inner-label trick this needs.
    Redo,
    /// `a, b = 1, 2` / `a, *b, c = arr` / `(a, b), c = ...` / `@x, $y, Z =
    /// ...` -- see `MultiTargetGroup`/`MultiTarget`'s docs for the full
    /// generalized target shape (local/ivar/cvar/global/const/attr/index/
    /// nested-group). See `zeo_rt::multi_assign`'s docs for the exact
    /// leniency rules (missing positions become `nil`; extra values are
    /// silently dropped when there's no splat to catch them).
    MultiWrite {
        targets: MultiTargetGroup,
        value: NodeId,
    },
    /// `eval("literal ruby source")` -- ONLY the compile-time-constant-string
    /// form (see `parse/mod.rs`'s eval-call-shape recognizer). `body`'s
    /// source was parsed and lowered into THIS SAME arena at lowering time --
    /// by the time `analyze`/`codegen` ever see this node, it's ordinary
    /// already-spliced Hir, indistinguishable from code written inline at the
    /// eval call site (so local/ivar scoping "just works" -- see the
    /// recognizer's docs). Runtime (non-literal) eval, `instance_eval`/
    /// `class_eval` with dynamic content, and `binding` are NOT implemented --
    /// see docs/EVAL_VM.md for the future embedded-interpreter design those
    /// would need.
    Eval(Vec<NodeId>),
    /// The body of a synthesized `attach_function` wrapper: marshal args,
    /// call the C symbol, wrap the result. See `FfiCall`.
    Ffi(Box<FfiCall>),
    /// A `Ruby::Box` context switch: the universal wrapper every
    /// box-scoped splice lowers into -- a `box.require`d file's statements,
    /// a `box.eval` body, and a `box::X` external-access expression all
    /// carry their statically-known box id here. Emits exactly like `Eval`
    /// (a brace-wrapped block whose value is the last statement's), except
    /// codegen's `Ctx.box_id` is overridden for the body -- the AOT
    /// translation of CRuby's loading-box/`cme->def->box` context.
    /// `analyze` descends top-level `BoxScope`s to register their
    /// `ClassDef`s under the box.
    BoxScope {
        box_id: u32,
        body: Vec<NodeId>,
    },
    /// The runtime VALUE of a box handle (`box = Ruby::Box.new` binds the
    /// local to this): a `RubyValue::Class` of the box's top-level
    /// surrogate class, so `p box` prints `#<Ruby::Box:N>`, handle equality
    /// works, and storing it is harmless. The four recognized OPERATION
    /// shapes (`box.require`/`box.load`/`box.eval`/`box::X`) resolve
    /// statically through the loader's bindings map, never through this
    /// value.
    BoxHandle(u32),
    /// `return` / `return value` -- explicit early return from the enclosing
    /// method. Compiles to a literal Rust `return Ok(value);` (Rust's own
    /// early return already exits arbitrarily deep nesting -- an `if`/`case`/
    /// loop body, or a fast-inline-path block like `.times`'s, which is
    /// spliced directly into the SAME enclosing method body, so a literal
    /// Rust `return` there already has real Ruby's exact semantics: `return`
    /// inside a block always exits the enclosing method, not just the
    /// block). This stops being correct only once a block can become a
    /// genuinely separate Rust closure (a real escaping `Proc`, a later
    /// phase) with its own Rust fn boundary a bare `return` would incorrectly
    /// stop at instead of passing through -- that needs `Signal::Return`
    /// (already reserved for exactly this in `zeo_rt::Signal`) once it
    /// exists. A multi-value `return a, b` lowers to a single implicit-array
    /// argument (`return [a, b]`), via `lower_single_optional_argument`, so
    /// this stays one optional node (mirroring `Break`/`Next`).
    Return(Option<NodeId>),
    /// `yield` / `yield(args)` -- invokes the enclosing method's implicit
    /// block.
    ///
    /// Recognized only directly within a method's own control flow, not
    /// inside a nested block literal. In real Ruby a `yield` lexically
    /// inside a block passed elsewhere refers to that block's own enclosing
    /// method; zeo does not attempt that case, and
    /// `analyze::register_class`'s `uses_bare_block` scan rejects it
    /// cleanly.
    ///
    /// Compiles to an invocation of the method's implicit `__blk` parameter
    /// (see `codegen::params`), raising real Ruby's `LocalJumpError` message
    /// when the method was called without a block.
    ///
    /// `Vec<ArrayElem>` is the shape `Call`'s positional arguments use, so
    /// `yield(*a)` needs no machinery of its own. Lowering folds a trailing
    /// `yield(k: 1)`/`yield(**h)` into one `HashLit`/merge element, which
    /// `codegen::params::emit_proc_param_bindings` binds a block's keyword
    /// params from.
    Yield(Vec<ArrayElem>),
    /// `block_given?` -- a zero-arg, no-receiver call-shape recognized at
    /// lowering time (mirrors `loop`/`define_method`'s desugars), not a
    /// distinct `ruby-prism` node. Same "not inside a nested block" scope-cut
    /// as `Yield`.
    BlockGiven,
    /// A bare `self` used as a VALUE (an explicit receiver, `self.foo`, or
    /// standalone, `puts self`) -- a real `ruby-prism` `SelfNode`, recognized
    /// generically (not a call-shape desugar). Only meaningful inside an
    /// ordinary instance method body (`cx.current_class` is `Some`, see
    /// `codegen::expr::infer`'s special case); a class method/module
    /// function has no backing instance to be (zeo has no first-class
    /// `Class`/`Module` runtime value), so
    /// `codegen::expr::emit_expr`'s `SelfRef` arm rejects that case with a
    /// clear error instead of emitting a reference to a Rust `self` that
    /// doesn't exist in that generated function's signature.
    SelfRef,
    /// `raise`/`fail` (exact synonyms) -- a zero/one/two-arg call-shape
    /// recognized at lowering time, same as `BlockGiven` above (real Ruby:
    /// both are ordinary `Kernel` method calls, not syntax). Codegen
    /// classifies the arg SHAPE itself (`ClassRef` vs. a `Str`-typed
    /// expression vs. an already-constructed exception value) rather than
    /// this node encoding it structurally -- mirrors `Yield`'s "let codegen,
    /// which already has full type-inference machinery, decide" posture.
    /// Bare `raise` (re-raise, zero args) needs a currently-handled
    /// exception context that doesn't exist until `rescue` does --
    /// a clean rejection until then, not a silent no-op.
    Raise(Vec<NodeId>, RaiseCause),
    /// `case subject; in PATTERN [if/unless GUARD] ... [else ...] end` --
    /// see `Pattern`/`PatternArm`'s docs. Arms are tested top to bottom,
    /// first match wins (same "not a native `match`" reasoning as
    /// `CaseWhen`, since a pattern's own class-check/destructure/guard logic
    /// can't be expressed as Rust structural patterns generically).
    /// `else_body: None` with no arm matching raises `NoMatchingPatternError`
    /// (see `codegen::patterns`'s docs) -- a real, distinct case from
    /// `Some(vec![])` (an explicit, empty `else` clause, which just yields
    /// `nil`).
    CaseIn {
        subject: NodeId,
        arms: Vec<PatternArm>,
        else_body: Option<Vec<NodeId>>,
    },
    /// `expr in pattern` -- a boolean one-liner: `true`/`false` for whether
    /// `pattern` matches, never raises on failure. Any binding the pattern
    /// makes on a SUCCESSFUL match still leaks into the enclosing method
    /// scope (matching real Ruby -- pattern variables behave like ordinary
    /// local assignment regardless of which branch of the enclosing code
    /// ends up reading them afterward).
    MatchPredicate {
        subject: NodeId,
        pattern: Pattern,
    },
    /// `expr => pattern` -- the rightward-assignment one-liner: raises
    /// `NoMatchingPatternError` if `pattern` doesn't match (mirrors a
    /// `case/in` with no `else`), otherwise evaluates to `nil` (real Ruby:
    /// this form's value is never used for anything but its binding/raising
    /// side effect).
    MatchRequired {
        subject: NodeId,
        pattern: Pattern,
    },
    /// `begin body rescue ... else ... ensure ... end` -- also the desugared
    /// form of a method body that's implicitly a `BeginNode` (a `def` with a
    /// bare `rescue`/`ensure` and no explicit `begin`/`end`, confirmed
    /// empirically via `Prism.parse`) and of `expr rescue fallback` (the
    /// modifier form, including inside an endless method) -- see
    /// `parse/mod.rs`'s recognizers, all of which produce this same shape.
    /// `rescues` are tested top to bottom, first matching clause wins; an
    /// unmatched raise propagates (a `?` at this node's own codegen site,
    /// mirroring every other fallible sub-expression -- see
    /// `codegen::exceptions::emit_begin`). `else_body: Some(_)` runs (and its
    /// value REPLACES `body`'s own) only when `body` completed with no
    /// exception, matching real Ruby. `ensure_body`, when present, always
    /// runs exactly once after everything else has settled -- including a
    /// `retry`-driven re-attempt of `body` -- never once per attempt.
    Begin {
        body: Vec<NodeId>,
        rescues: Vec<RescueClause>,
        else_body: Option<Vec<NodeId>>,
        ensure_body: Option<Vec<NodeId>>,
    },
    /// `retry` -- restarts the nearest enclosing `begin`'s own `body` from
    /// the top (an `ensure` that already ran does NOT re-run). Only valid
    /// lexically inside a `rescue` clause in real Ruby (a real, if rare,
    /// `SyntaxError` otherwise) -- zeo doesn't re-validate that
    /// positional restriction at lowering time; see
    /// `codegen::exceptions::emit_retry`'s docs for what happens to a
    /// mis-scoped one instead (an uncaught `Signal`, not silent wrongness).
    Retry,
    /// `$foo` read/write -- a flat, genuinely process-wide store (the
    /// `LazyLock<Mutex<_>>` pattern, same as `cvars`/the Symbol interner),
    /// needing no ancestor search at all: unlike `@@x`, there's exactly ONE
    /// global namespace, shared by every class and every thread. An unset
    /// global reads as `nil`, matching real Ruby (no `NameError`, unlike an
    /// unset constant -- see `ConstRead`'s docs).
    GlobalRead(String),
    GlobalWrite(String, NodeId),
    /// `Foo::BAR` -- an explicit, namespace-qualified constant READ
    /// (`ConstantPathNode`). The class name must already be a registered
    /// class/module (same rule `constant_name`'s other callers enforce); the
    /// owner search starts there (walking ITS ancestors, the same scheme as
    /// `ClassVarRead`'s ownership resolution -- see
    /// `codegen::expr::const_owner_id_opt`), unlike a bare constant (lowered as
    /// an ordinary `ClassRef`, which falls back to the LEXICALLY-enclosing
    /// class when used as a plain value -- see `codegen::expr`'s `ClassRef`
    /// docs). Unlike an ivar/cvar/global's "never assigned" -> `nil`
    /// convention, an unset constant raises a real `NameError` -- matches
    /// actual Ruby.
    QualifiedConstRead(String, String),
    /// A LENIENT constant read: `None` when never assigned, rather than a
    /// `NameError`. `scope: None` for a bare name, `scope: Some(class_name)`
    /// for `Foo::NAME`.
    ///
    /// Ordinary Ruby source never produces this. It exists only as
    /// `parse::lower_or_write`'s desugar for `CONST ||= value`. Real Ruby
    /// confirms the leniency is that narrow: `CONST ||= v` on a
    /// never-assigned constant quietly defines it, while `CONST += v` and
    /// `CONST &&= v` on the same constant still raise.
    ///
    /// `codegen::expr::emit_defined`'s `Defined` node cannot back this. It
    /// classifies syntax -- whether something was written as a constant read
    /// -- not whether the constant was ever assigned. So this needs its own
    /// `zeo_rt::const_get`-backed codegen.
    ConstReadOrNil(Option<String>, String),
    /// `NAME = value` / `Foo::NAME = value` -- `scope: None` for the bare
    /// form (owned by the LEXICALLY-enclosing class/module body it's written
    /// in, or `Object` at the top level -- exactly `ClassVarWrite`'s
    /// ownership scheme); `scope: Some(class_name)` for an explicit
    /// `Foo::NAME = value` (`ConstantPathWriteNode`), which always targets
    /// that NAMED class directly regardless of lexical position.
    ConstWrite {
        scope: Option<String>,
        name: String,
        value: NodeId,
    },
    /// `expr::NAME` where `expr` names no class the compiler can resolve --
    /// `self::OPTION_NAMES` in a `Struct.new` block, `adapter::GitExecuteError`
    /// on a parameter, `self.class::Reason`. The scope is an ordinary
    /// expression, evaluated first, and the constant is found on whatever it
    /// answers.
    ///
    /// `lenient` is `obj::NAME ||= v`'s read half, exactly as
    /// `ConstReadOrNil` is `Foo::NAME ||= v`'s: nil instead of a `NameError`
    /// when the name was never assigned, so the write half can define it. Only
    /// `||=` gets that leniency -- `+=` and `&&=` still raise.
    DynConstRead {
        scope: NodeId,
        name: String,
        lenient: bool,
    },
    /// `expr::NAME = value`. Writes the scope's OWN constant table (assignment
    /// never walks an ancestry) and answers `value`.
    ///
    /// Ruby allows this only where a plain `NAME = value` would also be legal:
    /// inside a method body `obj::NAME = v` is the `dynamic constant
    /// assignment` SyntaxError, and prism reports it before lowering ever runs.
    /// `obj::NAME ||= v` in a method is legal, though -- the check is on the
    /// plain form alone -- which is the shape act_as_attribute is built on.
    DynConstWrite {
        scope: NodeId,
        name: String,
        value: NodeId,
    },
    /// `BEGIN { ... }` -- its body runs before ANY main statement, and
    /// several run in source order (oracle-verified).
    ///
    /// A marker rather than a construct: `analyze` hoists these bodies to
    /// the front of `main_statements` and drops the node. Nothing downstream
    /// ever sees one, which is why there is no codegen arm for it.
    ///
    /// (`END { ... }` needs no node at all -- it is `at_exit` exactly,
    /// including the reverse-order rule, so lowering rewrites it into that
    /// call.)
    PreExec(Vec<NodeId>),
    /// `alias $new $old` -- makes `$new` name `$old`'s STORAGE. A real,
    /// bidirectional alias, not a copy: oracle-verified that writing either
    /// name is visible through the other, and that a later `$old = 9` shows
    /// up as `$new`. So it can't lower to `$new = $old`; the runtime
    /// resolves the indirection on every access (`zeo_rt::globals`).
    ///
    /// `(new, old)`. Unlike `alias` on a METHOD (which lowering resolves by
    /// cloning the DefMethod), this needs no target to exist yet: aliasing
    /// an unset global is legal and both names then read nil.
    AliasGlobal(String, String),
    /// `undef foo, bar` in a class/module body -- makes those names raise
    /// NoMethodError on this class, INCLUDING names it only inherits
    /// (oracle-verified: `class C < B; undef inherited_m; end` makes
    /// `C.new.inherited_m` a NoMethodError while `B.new.inherited_m` still
    /// works, and `C.new.respond_to?(:inherited_m)` is false).
    ///
    /// That inherited case is why this can't just delete a `DefMethod` from
    /// the class body at lowering time: there is no local def to delete.
    /// `analyze::register_class` records the names, and
    /// `mro::materialize_methods` then refuses to materialize them onto
    /// this class -- which removes them from the one table both dispatch
    /// paths and `respond_to?` consult.
    Undef(Vec<String>),
    /// `undef foo` / `undef_method :foo` written inside `class << self` -- the
    /// singleton half of [`HirNode::Undef`], which makes those names raise
    /// NoMethodError as CLASS methods of the enclosing class, inherited ones
    /// included (oracle-verified against `class << self; undef_method :new`,
    /// the shape optparse and rspec-mocks use to retire an inherited
    /// constructor). `ClassInfo::class_undefined` records them and
    /// `mro::materialize_class_methods` then refuses to materialize them.
    ClassMethodUndef(Vec<String>),
    /// `alias new old` / `alias_method :new, :old` where `old` is NOT defined
    /// earlier in the same class/module body -- an INHERITED method (or one a
    /// later reopen adds). Lowering can't clone the source `DefMethod` because
    /// it isn't in this body, and it can't resolve the ancestor chain either
    /// (ancestors are only linearized in `analyze`). So it records `(new,
    /// old)` here; `analyze::register_class` collects it into the class's
    /// `pending_aliases`, and `mro::resolve_aliases` (after ancestors are
    /// computed, before methods materialize) walks the MRO's `own_methods`,
    /// clones the source scope's params/body under `new`, and registers it --
    /// so it then materializes onto this class AND its subclasses normally.
    /// The same-body case stays a lowering-time `DefMethod` clone (no runtime
    /// target needed), exactly like `alias` on a locally-defined method.
    ///
    /// `is_class_method` marks an `alias` written inside `class << self` (`alias
    /// split shellsplit`): it aliases a SINGLETON method, so it resolves against
    /// the MRO's `own_class_methods` and registers as an own class method.
    AliasMethod {
        new_name: String,
        old_name: String,
        is_class_method: bool,
    },
    /// `private :m` / `public :m` / `protected :m` naming a method NOT defined
    /// earlier in the same class/module body -- an INHERITED method whose
    /// visibility this class re-declares (`class Sub < Base; private :base_pub;
    /// public :base_priv; end`). The same-body case is handled at lowering time
    /// by `set_method_visibility` on the local `DefMethod`; there is no local
    /// def here, so the name+visibility is recorded for `analyze::register_class`
    /// to collect into the class's `visibility_overrides`, applied after
    /// materialization stamps each method with its defining class's visibility.
    MethodVisibility {
        name: String,
        visibility: Visibility,
    },
    /// `private_class_method :m` / `public_class_method :m` naming a class
    /// method NOT defined earlier in the same body -- one inherited from the
    /// superclass, or `new` itself. The same-body case is retagged in place at
    /// lowering time; this variant defers the rest, exactly as
    /// [`HirNode::MethodVisibility`] does for the instance half.
    ClassMethodVisibility {
        name: String,
        visibility: Visibility,
    },
    /// `module_function :name` where `name` is INHERITED rather than defined
    /// in this body -- `erb/util.rb`'s `include ERB::Escape; module_function
    /// :html_escape`. A name the body does define is retagged in place at
    /// lowering time; this variant defers the rest to `mro`, which can only
    /// find the source once ancestors are linearized. The instance copy stays
    /// (the mixin half of `module_function`), and a module method is added
    /// alongside it.
    ModuleFunction(String),
    /// `private_constant :A, :B` / `public_constant :A` in a class body.
    ///
    /// Recognized at LOWERING time rather than left as a runtime call, because
    /// the reference it has to reject -- a qualified `M::A` from outside `M` --
    /// is one zeo resolves statically. Codegen compares the reading scope's
    /// cref against the owner and raises there; the runtime half only has to
    /// keep `Module#constants` and `defined?` honest.
    ConstantVisibility {
        names: Vec<String>,
        private: bool,
    },
    /// The last-match specials: `$~`, `$1`..`$9`, `$&`, `` $` ``, `$'`.
    ///
    /// NOT `GlobalRead`, even though they are spelled like globals: nothing
    /// ever assigns them (a regexp match does, as a side effect), and they
    /// read off a dedicated runtime slot rather than the `$foo` table. See
    /// `zeo_rt::lastmatch`, including the frame-locality divergence.
    LastMatchRef(LastMatch),
    /// A statement sequence evaluated in order, answering its LAST
    /// statement's value. `emit_body` emits it as one tail-value Rust block,
    /// the same shape `Eval` uses.
    ///
    /// Two sources reach here:
    ///   - a parenthesized multi-statement expression in real source
    ///     (`x = (a; b)`, `(puts "hi"; 42)`);
    ///   - lowering, which binds a compound-assignment target's receiver and
    ///     index expressions to a hidden local exactly once before reading
    ///     and writing through them (`obj.attr += 1`, `arr[i] ||= 1`). This
    ///     matches Ruby's evaluate-the-receiver-once rule; see
    ///     `parse::lower_call_operator_write`.
    ///
    /// It introduces no scope of its own. A local assigned inside stays
    /// readable afterwards, which is both what Ruby does with `(a; b)` and
    /// what the compound-assignment lowering needs from its hidden locals.
    ///
    /// Distinct from `Eval`, a literal `eval("...")`'s spliced body, because
    /// the two mean different things to a reader, not because they generate
    /// differently.
    Seq(Vec<NodeId>),
    /// `left..right` / `left...right` used AS a condition -- Ruby's
    /// flip-flop, a two-state latch rather than a Range.
    ///
    /// Off, it evaluates `left` and turns on when that is truthy. On, it
    /// evaluates `right` and turns off when that is truthy. Either way it
    /// answers true whenever it is, or just became, on. The two-dot form
    /// also tests `right` in the evaluation that turned it on, so
    /// `(i == 3)..(i == 3)` is true for one iteration. The three-dot form
    /// (`exclusive`) waits for the next one.
    ///
    /// prism mints this node only in a conditional position -- a `..`
    /// anywhere else is an ordinary `RangeLit` -- so no context flag is
    /// needed. An omitted side is nil, hence falsy: `..(i == 3)` never turns
    /// on and `(i == 2)..` never turns off, both oracle-verified.
    ///
    /// `state` indexes the runtime's latch table, minted per SYNTACTIC
    /// occurrence. That is what Ruby scopes the latch to: two flip-flops in
    /// one loop body keep separate state, and one flip-flop keeps its state
    /// across separate runs of its loop.
    FlipFlop {
        state: u32,
        left: NodeId,
        right: NodeId,
        exclusive: bool,
    },
}

impl HirNode {
    /// Whether ONLY a `class`/`module` body can hold this node.
    ///
    /// These are the class-body directives: the lowerer emits them from the
    /// static class path, and codegen consumes them there and nowhere else. A
    /// class built at RUNTIME (`Class.new { ... }`, a `class_eval` reopen) runs
    /// its body as an ordinary block, so each one has to be rewritten into the
    /// equivalent self-send first -- see
    /// `lower::defs::transform_runtime_class_body`, which refuses to pass an
    /// unrewritten directive through.
    ///
    /// The two halves used to be independent lists and drifted apart silently:
    /// a directive added to the static path but not the runtime one reached
    /// codegen as a "top-level-only node in expression position". Exhaustive
    /// here, with no `_` catch-all, so a new variant has to be classified.
    pub fn is_class_body_directive(&self) -> bool {
        match self {
            HirNode::Include(_)
            | HirNode::Extend(_)
            | HirNode::Prepend(_)
            | HirNode::ClassMethodPrepend(_)
            | HirNode::Refine { .. }
            | HirNode::Undef(_)
            | HirNode::ClassMethodUndef(_)
            | HirNode::AliasMethod { .. }
            | HirNode::MethodVisibility { .. }
            | HirNode::ClassMethodVisibility { .. }
            | HirNode::ConstantVisibility { .. }
            | HirNode::ModuleFunction(_) => true,

            // `ClassDef` and `DefMethod` are class-body shapes too, but both
            // also stand on their own at the top level and inside a method,
            // and codegen emits them in block position -- the runtime path
            // rewrites the former only to give it a runtime superclass.
            HirNode::ClassDef { .. }
            | HirNode::DefMethod { .. }
            | HirNode::Program(_)
            | HirNode::IntegerLit(_)
            | HirNode::BigIntegerLit { .. }
            | HirNode::RationalLit { .. }
            | HirNode::ImaginaryLit(_)
            | HirNode::FloatLit(_)
            | HirNode::SymbolLit(_)
            | HirNode::NilLit
            | HirNode::BoolLit(_)
            | HirNode::And(..)
            | HirNode::Or(..)
            | HirNode::Defined(_)
            | HirNode::If { .. }
            | HirNode::CaseWhen { .. }
            | HirNode::ArrayLit(_)
            | HirNode::HashLit(_)
            | HirNode::RangeLit { .. }
            | HirNode::StringLit(_)
            | HirNode::RegexpLit(..)
            | HirNode::LocalRead(_)
            | HirNode::LocalWrite(..)
            | HirNode::IvarRead(_)
            | HirNode::IvarWrite(..)
            | HirNode::ClassVarRead(_)
            | HirNode::ClassVarWrite(..)
            | HirNode::ClassRef(_)
            | HirNode::Call { .. }
            | HirNode::New { .. }
            | HirNode::SuperCall { .. }
            | HirNode::Block { .. }
            | HirNode::Lambda { .. }
            | HirNode::While { .. }
            | HirNode::Loop { .. }
            | HirNode::For { .. }
            | HirNode::Break(_)
            | HirNode::Next(_)
            | HirNode::Redo
            | HirNode::MultiWrite { .. }
            | HirNode::Eval(_)
            | HirNode::Ffi(_)
            | HirNode::BoxScope { .. }
            | HirNode::BoxHandle(_)
            | HirNode::Return(_)
            | HirNode::Yield(_)
            | HirNode::BlockGiven
            | HirNode::SelfRef
            | HirNode::Raise(..)
            | HirNode::CaseIn { .. }
            | HirNode::MatchPredicate { .. }
            | HirNode::MatchRequired { .. }
            | HirNode::Begin { .. }
            | HirNode::Retry
            | HirNode::GlobalRead(_)
            | HirNode::GlobalWrite(..)
            | HirNode::QualifiedConstRead(..)
            | HirNode::ConstReadOrNil(..)
            | HirNode::ConstWrite { .. }
            | HirNode::DynConstRead { .. }
            | HirNode::DynConstWrite { .. }
            | HirNode::PreExec(_)
            | HirNode::AliasGlobal(..)
            | HirNode::LastMatchRef(_)
            | HirNode::Seq(_)
            // `using` stands on its own at the top level far more often than
            // in a class body, and it edits nothing about the class -- it is
            // an ordinary statement whose only effect is compile-time.
            | HirNode::Using(_)
            // A definition REPORT is not a directive: it edits nothing about
            // the class and runs as an ordinary send at its position, which is
            // exactly what the runtime-class-body rewrite wants of it.
            | HirNode::DefHook { .. }
            | HirNode::MethodRedefine { .. }
            | HirNode::FlipFlop { .. } => false,
        }
    }

    /// Every child node this one owns, in evaluation order.
    ///
    /// The name-collecting passes (`analyze::collect_ivars`,
    /// `mro::collect_cvars`, `mro::collect_const_refs`) each used to carry
    /// their own copy of this walk, and the copies drifted: one missed a
    /// call's keyword arguments, another a `rescue *errs` splat, a third a
    /// block parameter's default. Every such omission is a silently WRONG
    /// answer, never a crash. One exhaustive match with no `..` rest pattern
    /// is what makes a new variant -- or a new field on an existing one --
    /// a compile error instead.
    ///
    /// `ClassDef`/`DefMethod` bodies are children like any other. A pass that
    /// must stop at a fresh Ruby scope matches those variants ahead of its
    /// fall-through to here.
    pub fn for_each_child(&self, visit: &mut impl FnMut(NodeId)) {
        fn each(ids: impl IntoIterator<Item = NodeId>, visit: &mut impl FnMut(NodeId)) {
            ids.into_iter().for_each(visit);
        }
        fn elems(es: &[ArrayElem], visit: &mut impl FnMut(NodeId)) {
            for e in es {
                let (ArrayElem::Single(n) | ArrayElem::Splat(n)) = e;
                visit(*n);
            }
        }
        fn kws(ks: &[KwArg], visit: &mut impl FnMut(NodeId)) {
            ks.iter().flat_map(|kw| kw.node_ids()).for_each(visit);
        }
        fn defaults(p: &Params, visit: &mut impl FnMut(NodeId)) {
            p.default_ids().into_iter().for_each(visit);
        }
        fn parts(ps: &[StrPart], visit: &mut impl FnMut(NodeId)) {
            for p in ps {
                if let StrPart::Interp(n) = p {
                    visit(*n);
                }
            }
        }
        match self {
            HirNode::Program(body)
            | HirNode::Eval(body)
            | HirNode::PreExec(body)
            | HirNode::Seq(body)
            | HirNode::Loop { body }
            | HirNode::BoxScope { box_id: _, body } => each(body.iter().copied(), visit),
            HirNode::Ffi(call) => call.args.iter().for_each(|(a, _)| visit(*a)),
            HirNode::ImaginaryLit(inner) => visit(*inner),
            HirNode::And(l, r)
            | HirNode::Or(l, r)
            | HirNode::FlipFlop {
                state: _,
                left: l,
                right: r,
                exclusive: _,
            } => {
                visit(*l);
                visit(*r);
            }
            HirNode::Defined(v) => visit(*v),
            HirNode::If {
                cond,
                then_body,
                else_body,
            } => {
                visit(*cond);
                each(then_body.iter().copied(), visit);
                each(else_body.iter().copied(), visit);
            }
            HirNode::CaseWhen {
                subject,
                arms,
                else_body,
            } => {
                each(subject.iter().copied(), visit);
                for (values, body) in arms {
                    elems(values, visit);
                    each(body.iter().copied(), visit);
                }
                each(else_body.iter().copied(), visit);
            }
            HirNode::ArrayLit(es) | HirNode::Yield(es) => elems(es, visit),
            HirNode::HashLit(pairs) => kws(pairs, visit),
            HirNode::RangeLit {
                start,
                end,
                exclusive: _,
            } => each(start.iter().chain(end).copied(), visit),
            HirNode::StringLit(ps) | HirNode::RegexpLit(ps, _) => parts(ps, visit),
            HirNode::LocalWrite(_, value)
            | HirNode::IvarWrite(_, value)
            | HirNode::ClassVarWrite(_, value)
            | HirNode::GlobalWrite(_, value)
            | HirNode::ConstWrite {
                scope: _,
                name: _,
                value,
            } => visit(*value),
            HirNode::DynConstRead {
                scope,
                name: _,
                lenient: _,
            } => visit(*scope),
            HirNode::DynConstWrite {
                scope,
                name: _,
                value,
            } => {
                visit(*scope);
                visit(*value);
            }
            HirNode::Call {
                receiver,
                name: _,
                args,
                kwargs,
                block,
                block_arg,
                safe: _,
            } => {
                each(receiver.iter().copied(), visit);
                elems(args, visit);
                kws(kwargs, visit);
                each(block.iter().chain(block_arg).copied(), visit);
            }
            HirNode::New {
                class_name: _,
                args,
                kwargs,
                block,
            } => {
                each(args.iter().copied(), visit);
                kws(kwargs, visit);
                each(block.iter().copied(), visit);
            }
            HirNode::SuperCall {
                args,
                kwargs,
                zsuper: _,
                block,
                block_arg,
            } => {
                args.iter().for_each(|a| visit(a.node_id()));
                kws(kwargs, visit);
                each(block.iter().chain(block_arg).copied(), visit);
            }
            HirNode::Block { params: p, body } => {
                defaults(p, visit);
                each(body.iter().copied(), visit);
            }
            HirNode::Lambda {
                params: p,
                body,
                method_body: _,
            } => {
                defaults(p, visit);
                each(body.iter().copied(), visit);
            }
            HirNode::ClassDef {
                name: _,
                superclass: _,
                body,
                is_module: _,
            } => each(body.iter().copied(), visit),
            HirNode::DefMethod {
                name: _,
                params: p,
                body,
                is_class_method: _,
                visibility: _,
                is_def: _,
            } => {
                defaults(p, visit);
                each(body.iter().copied(), visit);
            }
            HirNode::While {
                cond,
                body,
                negate: _,
                post: _,
            } => {
                visit(*cond);
                each(body.iter().copied(), visit);
            }
            HirNode::For {
                target,
                iterable,
                body,
            } => {
                target.for_each_node(visit);
                visit(*iterable);
                each(body.iter().copied(), visit);
            }
            HirNode::MultiWrite { targets, value } => {
                targets.for_each_node(visit);
                visit(*value);
            }
            HirNode::Break(v) | HirNode::Next(v) | HirNode::Return(v) => {
                each(v.iter().copied(), visit)
            }
            HirNode::Raise(args, cause) => {
                each(args.iter().copied(), visit);
                each(raise_cause_node(cause), visit);
            }
            HirNode::CaseIn {
                subject,
                arms,
                else_body,
            } => {
                visit(*subject);
                for arm in arms {
                    arm.pattern.for_each_node(visit);
                    arm.guard.iter().for_each(|(g, _)| visit(*g));
                    each(arm.body.iter().copied(), visit);
                }
                else_body
                    .iter()
                    .for_each(|b| each(b.iter().copied(), visit));
            }
            HirNode::MatchPredicate { subject, pattern }
            | HirNode::MatchRequired { subject, pattern } => {
                visit(*subject);
                pattern.for_each_node(visit);
            }
            HirNode::Begin {
                body,
                rescues,
                else_body,
                ensure_body,
            } => {
                each(body.iter().copied(), visit);
                for r in rescues {
                    each(r.splats.iter().copied(), visit);
                    each(r.body.iter().copied(), visit);
                }
                else_body
                    .iter()
                    .chain(ensure_body)
                    .for_each(|b| each(b.iter().copied(), visit));
            }
            HirNode::IntegerLit(_)
            | HirNode::BigIntegerLit {
                negative: _,
                digits: _,
            }
            | HirNode::RationalLit {
                negative: _,
                num_digits: _,
                den_digits: _,
            }
            | HirNode::FloatLit(_)
            | HirNode::SymbolLit(_)
            | HirNode::NilLit
            | HirNode::BoolLit(_)
            | HirNode::BoxHandle(_)
            | HirNode::SelfRef
            | HirNode::BlockGiven
            | HirNode::Redo
            | HirNode::Retry
            | HirNode::LocalRead(_)
            | HirNode::IvarRead(_)
            | HirNode::ClassVarRead(_)
            | HirNode::ClassRef(_)
            | HirNode::GlobalRead(_)
            | HirNode::LastMatchRef(_)
            | HirNode::QualifiedConstRead(_, _)
            | HirNode::ConstReadOrNil(_, _)
            | HirNode::Include(_)
            | HirNode::Extend(_)
            | HirNode::Prepend(_)
            | HirNode::ClassMethodPrepend(_)
            | HirNode::Refine { .. }
            | HirNode::Using(_)
            | HirNode::DefHook { .. }
            | HirNode::MethodRedefine { .. }
            | HirNode::Undef(_)
            | HirNode::ClassMethodUndef(_)
            | HirNode::AliasGlobal(_, _)
            | HirNode::AliasMethod {
                new_name: _,
                old_name: _,
                is_class_method: _,
            }
            | HirNode::MethodVisibility {
                name: _,
                visibility: _,
            }
            | HirNode::ClassMethodVisibility {
                name: _,
                visibility: _,
            }
            | HirNode::ModuleFunction(_)
            | HirNode::ConstantVisibility { .. } => {}
        }
    }
}

#[cfg(test)]
mod span_tests {
    use super::{FileId, Hir, HirNode, Span};

    /// Every pushed node gets a span row; nodes lowered inside a registered
    /// file point at their own source bytes, innermost node winning.
    #[test]
    fn lowering_stamps_byte_precise_spans_parallel_to_nodes() {
        let src = "x = 1\nputs x + 2\n";
        let mut hir = Hir::default();
        let file = hir.add_file("app.rb", src);
        hir.lowering_file = Some(file);
        let stmts = crate::lower::parse_and_lower_into(&mut hir, src).expect("lowers");
        hir.lowering_file = None;
        assert_eq!(hir.nodes().len(), hir.iter().count());

        // The `IntegerLit(2)` node's span covers exactly the `2` byte.
        let two = (0..hir.nodes().len())
            .map(|i| super::NodeId(i as u32))
            .find(|&id| matches!(hir[id], HirNode::IntegerLit(2)))
            .expect("the literal 2 was lowered");
        let span = hir.span(two).expect("a real span");
        assert_eq!(span.file, file);
        assert_eq!(&src[span.start as usize..span.end as usize], "2");

        // The statement roots carry their full statement ranges.
        let first = hir.span(stmts[0]).expect("a real span");
        assert_eq!(&src[first.start as usize..first.end as usize], "x = 1");
    }

    /// Without a registered file (the exception prelude, `eval` bodies) the
    /// same lowering stamps only `SYNTH`, and `span()` answers `None`.
    #[test]
    fn lowering_without_a_file_stamps_no_provenance() {
        let mut hir = Hir::default();
        let stmts = crate::lower::parse_and_lower_into(&mut hir, "a = [1, 2]\n").expect("lowers");
        assert!(stmts.iter().all(|&id| hir.span(id).is_none()));
    }

    /// An error surfacing from deep inside a statement carries the span of
    /// the innermost offending construct, not the whole statement.
    #[test]
    fn a_lowering_error_pinpoints_the_offending_construct() {
        let src = "y = 1\nputs(1) if /bad/\n";
        let mut hir = Hir::default();
        let file = hir.add_file("app.rb", src);
        hir.lowering_file = Some(file);
        let err = crate::lower::parse_and_lower_into(&mut hir, src)
            .expect_err("a bare condition regexp is rejected");
        let span = err.span.expect("located");
        assert_eq!(span.file, file);
        assert_eq!(&src[span.start as usize..span.end as usize], "/bad/");
    }

    /// `SYNTH` round-trips as "no span" without an `Option` in the table.
    #[test]
    fn synth_is_not_a_known_span() {
        assert_eq!(Span::SYNTH.known(), None);
        let real = Span {
            file: FileId(3),
            start: 5,
            end: 9,
        };
        assert_eq!(real.known(), Some(real));
    }
}
