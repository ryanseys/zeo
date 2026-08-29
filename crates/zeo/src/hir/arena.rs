use super::*;

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
    /// `Compiler::accessor_shape`.
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
    /// An `If` that `try_conditional_reopen` SYNTHESIZED around a whole class
    /// body when it pushed a `class X ... end if cond` guard inside. The
    /// condition was written OUTSIDE the body, so `clif::collect::split_guard`
    /// has to lift it back out of the class-body function -- its locals and its
    /// `self` are the enclosing scope's. A guard the source wrote inside the
    /// body carries no flag and stays put, where `self` is the class.
    pub const HOISTED_CLASS_GUARD: NodeFlag = NodeFlag(1 << 10);
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
    /// The FFI declaration vocabulary -- see [`FfiVocab`].
    pub ffi: FfiVocab,
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
    /// (`clif/emit.rs`'s `inline_markers`), which a bit array cannot do without
    /// scanning the whole arena. Push order also makes that walk deterministic,
    /// which iterating a `HashSet` never was.
    block_bodied_def_list: Vec<NodeId>,
    /// Every constant PATH the program assigns, mapped to the value nodes
    /// assigned to it -- see [`Hir::const_write_values_in_scope`].
    ///
    /// The key is the FULL cref path a scope-less write lands on
    /// (`"Bundler::Settings::Path"` for a `Path = ...` inside `class
    /// Settings`), and the written spelling for an explicit `M::D = ...`.
    /// It used to be the bare leaf, and that was a whole-program collision:
    /// bundler's `Bundler::Settings::Path = Struct.new(...) do ... end` made
    /// every `class X < Path` ANYWHERE in the program look like a subclass of
    /// a runtime-minted class, so `class Git < Path` in
    /// `bundler/source/git.rb` was silently rewritten to `Git =
    /// Class.new(Path)`. The compile-time class was then never declared and
    /// never revealed, while the runtime one answered `const_get` -- two
    /// classes under one name, and `Bundler::Source::Git` raised
    /// `uninitialized constant` in a program that defines it.
    ///
    /// [`Hir::record_class_def`] has always keyed by the cref path and
    /// [`Hir::class_defined_in_scope`] has always searched lexically; this is
    /// the same rule for the assignment half, and the two are read together.
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
    /// `clif/stmt.rs`'s `lower_stmts`, which groups consecutive entries under one
    /// `singleton class` frame.
    pub singleton_frame_stmts: crate::compiler::FMap<NodeId, NodeId>,
    /// Every `DefMethod` the `attr_*` fold generated, mapped to the macro that
    /// wrote it. A class can `extend` a module defining its own
    /// `attr_accessor`, and then the macro is an ordinary method call ruby
    /// dispatches -- the fold has to give way. The FIRST node of one statement
    /// carries the call to put back; the rest carry `None` and are dropped
    /// with it. See `lower::defs::directives::attr`.
    pub attr_macro: crate::compiler::FMap<NodeId, Option<(String, Vec<String>)>>,
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
    /// A call whose METHOD NAME sits on a later line than the expression
    /// it belongs to (`recv\n  .m`, `end.m(..)`), by the name's byte
    /// offset. Ruby's backtrace reports the call at the NAME's line while
    /// coverage still counts the statement's first line -- oracle-verified,
    /// the two are different questions -- so this cannot be folded into the
    /// node's span, which answers the second.
    call_message: crate::compiler::FMap<NodeId, u32>,
    /// The `require`/loader state -- see [`LoaderState`].
    pub loader: LoaderState,
    /// In-tree `ext/` features (`zeo_abi::is_ext_feature`) whose `require`
    /// fired anywhere in the program -- the set that makes a require-gated
    /// builtin's constant REGISTER at all (`Compiler::resolve_class`'s feature
    /// gate). Whole-program AOT, so this set is program-GLOBAL: a feature no
    /// file requires registers nowhere. WHERE the constant starts existing is
    /// the other question, and a positional one -- `HirNode::FeatureLoaded`
    /// reveals it at the require's own line. See `activate_feature`.
    pub activated_features: crate::compiler::FSet<String>,
    /// The package owning the file currently lowering, `None` for the main
    /// file and the `-I` roots, and that file's own directory -- the pair
    /// `demand_feature_units` records. The directory is what a
    /// `File.expand_path("x", __dir__)` target is relative to, which is how
    /// stdlib and bundler spell a sibling autoload.
    pub lowering_package: Option<String>,
    pub lowering_dir: Option<std::path::PathBuf>,
    /// How many of the root `Program`'s leading statements came from the
    /// built-in exception classes (`parse::BUILTIN_EXCEPTIONS_RB`), set by
    /// `parse_and_lower_with`. `analyze` marks the classes
    /// those statements register as `is_bootstrap` -- the AOT analogue of
    /// CRuby's "defined before any user program runs" set, which stays
    /// visible inside every `Ruby::Box` (see `Compiler::resolve_class`'s
    /// bootstrap fallback).
    pub builtin_exceptions_len: usize,

    /// What this compile is FOR (`CompileMode`). Every static
    /// decision the emitter makes belongs to a whole PROGRAM, which owns
    /// the class table it registers into; a snippet compiled for a
    /// run-time `eval` arrives after that program is already running, so
    /// its own `def`s and `class`es install through the runtime instead.
    pub mode: crate::CompileMode,
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
    /// Every `class`/`module` definition lowered so far, under the FULLY
    /// QUALIFIED name its site spells (`class Error` inside `module Citrus`
    /// records `Citrus::Error`). A `ClassDef` node keeps only the name as
    /// WRITTEN, so an arena scan for one cannot tell citrus's `Citrus::Error`
    /// from a `TomlRB::Error` that is really a `Class.new` value -- and
    /// answering that wrong sends a subclass down the wrong path. See
    /// [`class_defined_in_scope`](Self::class_defined_in_scope).
    class_def_paths: crate::compiler::FSet<String>,
}

/// [`Hir`]'s FFI declaration vocabulary -- what the FFI directives
/// (`typedef`/`enum`/`layout`/`attach_function` and the const idioms)
/// declare during lowering, read back when later directives resolve names.
#[derive(Clone, Default)]
pub struct FfiVocab {
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
    /// Whether the inline-array proxy classes an `FFI::Struct` array field
    /// reads back as have already been synthesized -- see
    /// [`claim_ffi_inline_array_classes`](Hir::claim_ffi_inline_array_classes).
    ffi_inline_array_classes: bool,
}

/// [`Hir`]'s loader state -- what `require` resolution spliced, what is
/// compiled in as feature units, and what stays a runtime `require`.
#[derive(Clone, Default)]
pub struct LoaderState {
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
    /// What the runtime `$LOAD_PATH` holds: the compile-time require-search
    /// roots (`-I` + `RUBYLIB`) in search order, then the roots of every gem a
    /// `require` actually activated -- CRuby's own rule, where RubyGems adds a
    /// gem's lib directory when it activates it.
    ///
    /// Mostly cosmetic, since every require was resolved at compile time, but
    /// code that READS the array needs real directories in it: rspec's
    /// `RubyProject` inspects it, and `IRB::Locale#find` scans it with
    /// `File.readable?` for a file it then `Kernel.load`s.
    pub search_roots: Vec<String>,
    /// How many leading `search_roots` entries a RUN-TIME require may search
    /// (see `zeo_abi::ProgramDesc::n_load_path_search`). The `-I` roots are
    /// searchable; the bundled-gem roots after them are not.
    pub search_root_count: usize,
    /// `--embed-sources`: `(load-path-relative spelling, text)` for every
    /// `.rb` under the named directories. The RUN TIME resolves a require
    /// against these before it looks at disk, which is what lets a hermetic
    /// binary answer a require its compiler could not.
    pub embedded_sources: Vec<(String, String)>,
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
    /// Call sites whose target this compile had ALREADY spliced when the
    /// statement holding them was lowered. `require` answers `false` for a
    /// feature that is already loaded, and the fold that turns a resolvable
    /// literal require into a boolean cannot see which file a name resolves
    /// to -- the loader marks them (`Loader::already_spliced`) before the
    /// fold runs.
    pub rerequire_sites: crate::compiler::FSet<(FileId, u32)>,
    /// Single files to compile as feature units: (owning package, path,
    /// feature name as required). The per-file companion to `unit_demand`'s
    /// per-directory walk, for a conditional require whose one target is
    /// known.
    pub single_unit_demand:
        std::collections::BTreeSet<(Option<String>, std::path::PathBuf, String)>,
    /// The LEAF name of every constant a literal `autoload` names whose
    /// target is compiled in as a unit -- `Platform` for
    /// `Gem::Platform`.
    ///
    /// Such a constant is in the tables from startup, so a read never misses
    /// and no hook can run the unit. The emitter gates the read itself: see
    /// `zeo_rt_autoload_touch`. Without the gate the row had to load at the
    /// DECLARATION, which ran a target before the statements above it.
    ///
    /// The LEAF, not the qualified path, and deliberately. The lexical scope
    /// an `autoload` was written in is not always recoverable: a unit is
    /// compiled from a SLICE of its file, so the same declaration is seen
    /// once as `Gem::Platform` and once as a bare `Platform`, and a
    /// path-keyed set then missed the read that mattered. The name alone is
    /// enough to be safe because the runtime decides: `zeo_rt_autoload_touch`
    /// looks the exact `(owner, name)` pair up and does nothing when no
    /// autoload is pending for it. So this set only has to be an
    /// OVER-approximation, and the cost of a spare entry is one relaxed load.
    pub autoload_consts: std::collections::BTreeSet<String>,
    /// The features those constants' targets name -- how `materialize_units`
    /// tells an autoload unit from any other single-file demand.
    pub autoload_features: std::collections::BTreeSet<String>,
    /// Constants an autoload unit's body ASSIGNS. They do not exist until the
    /// unit runs, so `defined?` must ask at run time rather than answer from
    /// the write the compiler can see. A class the unit defines is a
    /// different case -- it is registered from startup, and
    /// [`autoload_consts`](Self::autoload_consts) gates the read of it.
    pub unrun_unit_consts: std::collections::BTreeSet<String>,
    /// Load paths to compile in WHOLE, as callable units rather than splices --
    /// keyed by owning package name, `None` for the `-I`/main roots. A file
    /// lands here when it computes a `require`/`autoload` target zeo cannot
    /// fold (`ActiveSupport::Autoload#autoload` joins the module name to the
    /// constant and calls `super`), so the only honest answer is to compile in
    /// everything that string could name. See `parse::loader::materialize_units`.
    pub unit_demand: std::collections::BTreeSet<(Option<String>, std::path::PathBuf)>,
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
    /// Deferred features that are BOTH a gated builtin and a resolvable gem
    /// -- `tmpdir` and `time` are the shape: the ext half supplies the
    /// constants, the GEM half supplies the Ruby code (`Dir::Tmpname`).
    ///
    /// Both halves are needed, so the builtin fold in `lower::calls` must
    /// not eat the call: the ext activates positionally and the call stands
    /// as a real runtime require that loads the gem. A PURE builtin is not
    /// here, and keeps folding -- which is the difference
    /// `is_builtin_feature` alone cannot see.
    pub dual_homed_requires: crate::compiler::FSet<String>,
}

/// One compiled-in load-path file -- see `LoaderState::feature_units`. Its statements
/// are NOT part of the main statement list: codegen emits them as a function
/// the runtime calls when a `require` names `feature`. Registration is
/// unaffected -- the classes it defines are in the dispatch tables from
/// startup, exactly as a spliced file's are.
#[derive(Clone)]
pub struct FeatureUnit {
    /// The name a `require` spells: the path under its load-path root, with
    /// no `.rb`.
    pub feature: String,
    /// Every OTHER load-path spelling that names this same file. One file
    /// can be demanded more than once -- rack's `autoload :MediaType,
    /// "rack/media_type"` and a guarded `require_relative
    /// "../lib/rack/media_type"` are the same file under two names -- and a
    /// unit that answers to only one of them leaves the other's require
    /// finding nothing.
    pub aliases: Vec<String>,
    /// The same file's absolute path, with no `.rb` -- the OTHER spelling a
    /// program can build, and the one `File.expand_path("x", __dir__)` (the
    /// `autoload` idiom stdlib and bundler use for a sibling file) produces.
    pub absolute: String,
    pub body: Vec<NodeId>,
}

/// One splice instance -- see `LoaderState::loaded_files`.
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
    /// Always 0 (the root box) for now -- see `LoaderState::loaded_files`.
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
    /// [`FfiVocab::ffi_types`]. Redeclaring one to a different type poisons it.
    pub fn declare_ffi_type(&mut self, name: &str, ty: &FfiType) {
        match self.ffi.ffi_types.get(name) {
            Some(Some(prev)) if prev == ty => {}
            Some(_) => {
                self.ffi.ffi_types.insert(name.to_string(), None);
            }
            None => {
                self.ffi
                    .ffi_types
                    .insert(name.to_string(), Some(ty.clone()));
            }
        }
    }

    /// The alias map a class body starts from: every unpoisoned name declared
    /// by an enclosing (or earlier) FFI library. Bodies lower in source order,
    /// so a nested struct sees what the module above it declared.
    pub fn inherited_ffi_types(&self) -> crate::compiler::FMap<String, FfiType> {
        let mut types: crate::compiler::FMap<String, FfiType> = self
            .ffi
            .ffi_types
            .iter()
            .filter_map(|(k, v)| v.clone().map(|t| (k.clone(), t)))
            .collect();
        // Struct layouts join the same table as `Struct` entries. What a
        // bare struct name MEANS depends on the position -- a signature
        // reads it back as `StructRef` (ruby-ffi's by-reference semantics),
        // a layout field keeps the inline by-value struct; see
        // `ffi_type_node`. An explicit typedef of the same name wins.
        for (k, layout) in &self.ffi.ffi_struct_layouts {
            types
                .entry(k.clone())
                .or_insert_with(|| FfiType::Struct(layout.clone()));
        }
        // A struct class whose `layout` zeo never saw (DSL-built) still
        // NAMES a struct; the by-reference entry serves every signature
        // position. Last, so a real layout or an explicit typedef wins.
        for k in &self.ffi.ffi_struct_classes {
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
        !std::mem::replace(&mut self.ffi.ffi_inline_array_classes, true)
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
    /// [`FfiVocab::ffi_layout_hooks`].
    pub(crate) fn ffi_layout_hook_for(&self, name: &str) -> Option<&String> {
        if let Some(absolute) = name.strip_prefix("::") {
            return self.ffi.ffi_layout_hooks.get(absolute);
        }
        for depth in (0..=self.cref_names.len()).rev() {
            let qualified = match depth {
                0 => name.to_string(),
                _ => format!("{}::{name}", self.cref_names[..depth].join("::")),
            };
            if let Some(source) = self.ffi.ffi_layout_hooks.get(&qualified) {
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
        self.ffi.ffi_library_crefs.insert(path.to_string());
    }

    pub(crate) fn is_ffi_library(&self, path: &str) -> bool {
        self.ffi.ffi_library_crefs.contains(path)
    }

    /// Records a `class X < FFI::Struct` by leaf name -- see
    /// [`FfiVocab::ffi_struct_classes`].
    pub(crate) fn mark_ffi_struct_class(&mut self, leaf: &str) {
        self.ffi.ffi_struct_classes.insert(leaf.to_string());
    }

    /// Whether `leaf` names a class already known to descend from
    /// `FFI::Struct` -- how `class B < A` is recognized as a struct in its own
    /// right. gssapi hides `[]`/`[]=` behind `GssUMStruct < FFI::Struct` and
    /// then declares every real struct against THAT, so nothing in the gem
    /// names `FFI::Struct` directly except the one intermediate.
    pub(crate) fn is_ffi_struct_class(&self, leaf: &str) -> bool {
        self.ffi.ffi_struct_classes.contains(leaf)
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
        match self.mode {
            crate::CompileMode::Program => self.cref_names.is_empty(),
            // An `eval` SNIPPET has no lexical cref of its OWN, and that is
            // not the same as being at the top level: its cref is the class
            // the caller was in. The caller knows whether there was one, so
            // the fold is right either way -- and a `class` body inside the
            // snippet opens a cref `cref_names` tracks like any other.
            crate::CompileMode::Eval { cref } => self.cref_names.is_empty() && !cref,
        }
    }
}

impl std::ops::Index<NodeId> for Hir {
    type Output = HirNode;
    fn index(&self, id: NodeId) -> &HirNode {
        &self.nodes[id.0 as usize]
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
                // An explicit `M::D = ...` keys by its written spelling: what
                // `M` resolves to is a lexical question lowering cannot
                // answer, and the lexical SEARCH below reaches this key at its
                // outermost candidate.
                Some(s) => format!("{s}::{name}"),
                // A scope-less write lands on the enclosing cref, so that is
                // its path -- the same rule `record_class_def` uses.
                None => self.cref_path(name),
            };
            self.const_writes.entry(key).or_default().push(*value);
        }
    }

    /// Every constant path something assigns so far, in the spelling
    /// [`const_writes`](Self::const_writes) keys by.
    pub fn const_write_names(&self) -> impl Iterator<Item = &String> {
        self.const_writes.keys()
    }

    /// The values assigned so far to the constant `name` names FROM THE CREF
    /// BEING LOWERED -- Ruby's lexical search, innermost scope first, then the
    /// top level; a `::`-anchored name asks at the top level only.
    ///
    /// The exact twin of [`class_defined_in_scope`](Self::class_defined_in_scope),
    /// and read together with it: the two answer "is this name a value-holding
    /// constant?" and "is it a compile-time class?", and a definition is
    /// routed to the runtime path only when the first says yes and the second
    /// says no. Asking the first WITHOUT the scope walk is what made
    /// `Bundler::Settings::Path` answer for `Bundler::Source::Path` -- see
    /// [`const_writes`](Self::const_writes).
    pub fn const_write_values_in_scope(&self, name: &str) -> &[NodeId] {
        if let Some(absolute) = name.strip_prefix("::") {
            return self.const_write_values(absolute);
        }
        (0..=self.cref_names.len())
            .rev()
            .map(|depth| match depth {
                0 => name.to_string(),
                _ => format!("{}::{name}", self.cref_names[..depth].join("::")),
            })
            .find_map(|candidate| self.const_writes.get(&candidate))
            .map_or(&[], Vec::as_slice)
    }

    /// The values assigned to the exact key `name`, in push order -- empty
    /// when nothing lowered yet assigns it. Callers that ask about a name a
    /// program WROTE want [`const_write_values_in_scope`](Self::const_write_values_in_scope);
    /// this is the raw table access behind it.
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

    /// Whether the string literal at `id` is FROZEN: the magic comment is
    /// per FILE (a required file's setting is its own), falling back to the
    /// main file's for a synthetic node.
    pub fn literal_frozen_at(&self, id: NodeId) -> bool {
        self.span(id)
            .and_then(|s| (s.file.0 != u32::MAX).then_some(s.file))
            .and_then(|f| self.files.get(f.0 as usize))
            .map_or(self.frozen_string_literal, |sf| sf.frozen_string_literal)
    }

    /// The provenance of `id` -- `None` for a synthetic node (see `Span`).
    pub fn span(&self, id: NodeId) -> Option<Span> {
        self.spans[id.0 as usize].known()
    }

    /// Record a call whose method name is on a later line than the
    /// expression's first (see [`Hir::call_message`]).
    pub(crate) fn set_call_message(&mut self, id: NodeId, offset: u32) {
        self.call_message.insert(id, offset);
    }

    /// The LINE a call's method name sits on, when that is not the line
    /// its expression starts on. `None` for every ordinary call, which is
    /// what keeps this to one entry per `recv\n  .m` in the program.
    pub fn call_line(&self, id: NodeId) -> Option<u32> {
        let offset = *self.call_message.get(&id)?;
        let file = self.files.get(self.span(id)?.file.0 as usize)?;
        Some(file.line_at(offset))
    }

    /// Records that the file currently lowering computes a `require`/`autoload`
    /// target, so its whole load path must be compiled in as units. Keyed by
    /// the file's owning package, which bounds the walk to that gem.
    pub fn demand_feature_units(&mut self) {
        let package = self.lowering_package.clone();
        let dir = self.lowering_dir.clone().unwrap_or_default();
        tracing::debug!(?package, ?dir, file = ?self.lowering_file, "demand_feature_units");
        if crate::debug_flags::debug(crate::debug_flags::DebugFlag::NoPackageSweep) {
            return;
        }
        self.loader.unit_demand.insert((package, dir));
    }

    /// The program's OWN file -- what a `<main>` frame names, what tells a
    /// spliced require's top level apart from it, and the DWARF unit name.
    ///
    /// Not `files[0]`. The vendored corelib registers its `<internal:>`
    /// sources ahead of the main file (see `parse::corelib`), so position
    /// stopped being the answer and three emitter sites started naming
    /// `<internal:nilclass>` as every `<main>` frame's file. `main_file` is
    /// unset for `-e`, which has no path, so the fallback skips the corelib
    /// segments rather than taking the first row.
    pub fn entry_file_name(&self) -> Option<&str> {
        if let Some(id) = self.main_file {
            return Some(self.files[id.0 as usize].name.as_str());
        }
        self.files
            .iter()
            .map(|f| f.name.as_str())
            .find(|n| !n.starts_with("<internal:"))
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

    /// The half of [`Compiler::needs_prism_runtime`] the `Hir` owns: the
    /// program `require`s prism itself, whose `ext-prism` module calls the
    /// same C library's serialize entry points.
    ///
    /// [`Compiler::needs_prism_runtime`]: crate::compiler::Compiler::needs_prism_runtime
    pub(crate) fn activates_prism(&self) -> bool {
        self.activated_features.contains("prism")
    }

    /// Whether the program names a `RubyVM` surface whose body parses at
    /// runtime (`AbstractSyntaxTree.parse`, `InstructionSequence.compile`) --
    /// those link the prism runtime exactly like a dynamic `eval` does.
    pub(crate) fn mentions_rubyvm_parser(&self) -> bool {
        self.nodes.iter().any(|node| match node {
            HirNode::ClassRef(n) => n == "RubyVM",
            HirNode::QualifiedConstRead(scope, n) => {
                n == "RubyVM" || scope == "RubyVM" || scope.starts_with("RubyVM::")
            }
            _ => false,
        })
    }

    /// Whether this program can reach a RUN-TIME `eval`.
    ///
    /// Only `Kernel#eval` and string-form `instance_eval` funnel into
    /// `zeo_rt::eval_value`/`eval_string`. EVERY `eval` is a run-time one:
    /// the compile-time splice a literal source once took was retired
    /// once a snippet compiled for real, because the splice reported the
    /// ENCLOSING file for `__FILE__` and every backtrace row, ignored a
    /// magic comment written in the string, and shared the caller's
    /// storage where CRuby shares a Binding. Block-form `instance_eval
    /// { ... }` runs a real block and carries it in `block`, so a
    /// POSITIONAL argument marks the string form.
    ///
    /// This scans the whole arena rather than traversing from the roots, so
    /// it also catches eval sites inside spliced files and method bodies.
    /// Over-approximation is safe: a false positive only links the compiler
    /// into a binary that never calls it. A miss -- `send(name, src)` with
    /// a computed name, which no static analysis can see -- raises the
    /// runtime's own `NotImplementedError` saying the program carries no
    /// compiler, never silent wrong output.
    /// The name of a `require`/`load` whose target this compile could not
    /// resolve, if there is one -- what `--strict-static-require` refuses.
    pub fn unresolved_require(&self) -> Option<&str> {
        self.nodes.iter().find_map(|node| match node {
            HirNode::Call { name, .. }
                if matches!(name.as_str(), "require" | "require_relative" | "load") =>
            {
                Some(name.as_str())
            }
            _ => None,
        })
    }

    /// The name that makes `id` an eval-shaped call, and whether it was
    /// written with an explicit RECEIVER -- [`Hir::uses_runtime_eval`]'s test
    /// for one node, so the narrowed answer and the flat scan cannot drift.
    pub fn eval_shaped(&self, id: NodeId) -> Option<(&str, bool)> {
        let HirNode::Call { name, receiver, .. } = &self[id] else {
            return None;
        };
        self.eval_shaped_node(&self[id])
            .then(|| (name.as_str(), receiver.is_some()))
    }

    fn eval_shaped_node(&self, node: &HirNode) -> bool {
        match node {
            HirNode::Call { name, args, .. } => match name.as_str() {
                "eval" => true,
                "instance_eval" | "class_eval" | "module_eval" => !args.is_empty(),
                "require" | "require_relative" | "load" => true,
                "send" | "__send__" | "public_send" => args
                    .first()
                    .and_then(|a| self.sent_name(a.node_id()))
                    .is_some_and(|n| {
                        matches!(n, "eval" | "instance_eval" | "class_eval" | "module_eval")
                    }),
                _ => false,
            },
            _ => false,
        }
    }

    pub fn uses_runtime_eval(&self) -> bool {
        self.nodes.iter().any(|node| match node {
            HirNode::Call { name, args, .. } => match name.as_str() {
                // Any `eval`, receiver or not: `Binding#eval` runs its source
                // through the same entry, and a Binding is an ordinary value
                // a call site can hold in anything.
                "eval" => true,
                "instance_eval" | "class_eval" | "module_eval" => !args.is_empty(),
                // A `require`/`load` that SURVIVED lowering is one the
                // loader could not resolve -- a computed target, or a file
                // under no compile-time root. It reaches the same compiler
                // at run time (`features::UnitCompiler`), so a program that
                // can reach one carries it.
                "require" | "require_relative" | "load" => true,
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

    /// Whether a constant's PRIVACY can change while the program runs.
    ///
    /// `private_constant`/`public_constant` in a class body are compile-time
    /// facts analyze folds into `ClassInfo::private_constants`; a `Module`
    /// reached through a dynamic send, or a snippet a run-time `eval`
    /// compiles, sets the same flag with nothing static to see. A program
    /// where either is possible asks the run time at every explicit-scope
    /// read instead of folding one -- which is why the predicate is a whole
    /// property rather than a per-site one: the directive and the read are
    /// in different scopes by construction.
    pub fn constant_privacy_is_runtime(&self) -> bool {
        self.uses_runtime_eval()
            || self.nodes.iter().any(|node| {
                matches!(node, HirNode::Call { name, .. }
                    if name == "private_constant" || name == "public_constant")
            })
    }

    /// Whether the program can observe a `:call`/`:return` event -- it names
    /// `TracePoint` or calls `set_trace_func` anywhere.
    ///
    /// Those two events are the only thing a devirtualized accessor stops
    /// producing (`clif/params.rs`'s `define_accessor` reaches the
    /// field with no frame and no callee at all), so the emitter keeps the
    /// ordinary call shape for a program that could watch for them -- the
    /// same "the instrumentation is ABSENT, not branched on" model
    /// the line-coverage emission uses.
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
    /// the moved-object guards: only a program that can reach
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
    /// `analyze::captures::binding_scope_names`). Memoized: every method
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
