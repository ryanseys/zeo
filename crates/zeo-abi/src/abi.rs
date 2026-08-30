//! The Cranelift-backend value ABI: the layout contract generated code and
//! the runtime share.
//!
//! `zeo-rt` makes `RubyValue` `#[repr(C, u8)]` with the explicit
//! discriminants in [`ValueTag`]; its `abi_layout` test asserts every
//! constant here against the real types, so the two sides cannot drift.
//! Generated code reads/writes ONLY the tag byte and the `Int`/`Float`/
//! `Bool`/`Symbol`/`Class` payloads inline; every heap payload is opaque
//! (retain/release/inspect via runtime calls).
//!
//! Numbering rule: the six no-`Arc` payloads (a 24-byte copy needs no
//! retain/release) sit below [`FIRST_HEAP_TAG`]; every `Arc`-backed payload
//! sits at or above it. Within each band, numbers follow the enum's source
//! order. APPEND-ONLY, like `ClassId`s.

/// `size_of::<RubyValue>()` -- and of the 24-byte value slots generated
/// code allocates.
pub const VALUE_SIZE: usize = 24;
/// `align_of::<RubyValue>()`.
pub const VALUE_ALIGN: usize = 8;
/// The tag byte's offset inside a value.
pub const TAG_OFFSET: usize = 0;
/// Every payload's offset: `repr(C, u8)` is tag + union, so `Int`'s `i64`,
/// `Float`'s `f64`, `Bool`'s `i8` and `Symbol`/`Class`'s `u32` all start
/// here.
pub const PAYLOAD_OFFSET: usize = 8;

/// `RubyValue`'s explicit discriminants. Immediates (no `Arc` payload)
/// below 16, heap variants from 16.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValueTag {
    Nil = 0,
    Bool = 1,
    Int = 2,
    Float = 3,
    Symbol = 4,
    Class = 5,
    BigInt = 16,
    Rational = 17,
    Complex = 18,
    Str = 19,
    Array = 20,
    Hash = 21,
    Range = 22,
    Object = 23,
    Proc = 24,
    Regexp = 25,
    MatchData = 26,
    Fiber = 27,
    Enumerator = 28,
    Yielder = 29,
    Thread = 30,
    Mutex = 31,
    Queue = 32,
    Ractor = 33,
}

/// `tag < FIRST_HEAP_TAG` => the value is a plain 24-byte memcpy, no
/// retain/release; `>=` => the payload holds an `Arc` the runtime must
/// retain/release.
pub const FIRST_HEAP_TAG: u8 = 16;

/// The highest tag [`ValueTag`] defines. A byte above it is not a tag at
/// all, which is how the runtime tells an uninitialised slot from a value.
pub const LAST_HEAP_TAG: u8 = ValueTag::Ractor as u8;

/// A compiled function returned normally; `out` holds the value.
pub const STATUS_OK: i32 = 0;
/// A signal is pending in the per-fiber slot; `out` is untouched.
pub const STATUS_SIGNAL: i32 = 1;

/// What the pending-signal slot holds -- `Signal`'s variants by number,
/// as `zeo_rt_signal_kind`/`zeo_rt_signal_take` answer them. `None` = the
/// slot is empty. `Terminate` is the fiber/enumerator teardown signal
/// (never native unwinding).
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignalKind {
    None = 0,
    Break = 1,
    Next = 2,
    Redo = 3,
    Retry = 4,
    Return = 5,
    Raise = 6,
    Throw = 7,
    Terminate = 8,
}

/// `size_of` of the runtime's `.bss` inline-cache site structs -- what the
/// emitter sizes each opaque site slot to (initialised by `zeo_unit_init`,
/// never assumed zero-valid). All align 8. Asserted by `zeo-rt`'s
/// `abi_layout` test. The three method-cache sites grew 8 bytes when their
/// cached entry widened from a bare `ValueMethodFn` to `ValueImpl` (the
/// Rust-or-C two-word implementation).
pub const CALLSITE_SIZE: usize = 56;
pub const DYNCALLER_SITE_SIZE: usize = 56;
pub const CLASSMETHOD_SITE_SIZE: usize = 48;
pub const CONST_SITE_SIZE: usize = 40;
pub const NEW_SITE_SIZE: usize = 32;
pub const CIVAR_SITE_SIZE: usize = 40;
pub const REGEXP_SITE_SIZE: usize = 16;
pub const FFISYM_SITE_SIZE: usize = 16;
/// The common alignment of every site struct above.
pub const SITE_ALIGN: usize = 8;

// --- The per-thread frame/pool hot header ------------------------------------
//
// `zeo_rt_frame_hot()` answers the address of the thread's `FrameHot`
// header (stable for the thread's lifetime); emitted prologues push and
// pop call frames, stamp lines and pool temporaries through these
// offsets. Pinned by `zeo-rt`'s `frame_hot_layout` test -- including the
// `&str` (ptr, len) fat-pointer layout the two text fields assume.

/// `FrameHot` field offsets: the frame trio, then the pool trio.
pub const FRAMEHOT_TOP: usize = 0;
pub const FRAMEHOT_BASE: usize = 8;
pub const FRAMEHOT_END: usize = 16;
pub const FRAMEHOT_POOL_TOP: usize = 24;
pub const FRAMEHOT_POOL_BASE: usize = 32;
pub const FRAMEHOT_POOL_END: usize = 40;

/// One `Frame`'s size and field offsets.
pub const FRAME_SIZE: usize = 48;
pub const FRAME_FILE_PTR: usize = 0;
pub const FRAME_FILE_LEN: usize = 8;
pub const FRAME_METHOD_PTR: usize = 16;
pub const FRAME_METHOD_LEN: usize = 24;
pub const FRAME_LINE: usize = 32;
pub const FRAME_END_LINE: usize = 36;
pub const FRAME_POOL_MARK: usize = 40;
/// `Frame.pool_mark`'s "no pool scope" sentinel.
pub const FRAME_NO_MARK: u32 = u32::MAX;

/// The `zeo_rt_gates` bit emitted frame prologues test
/// (`GATE_FRAMES_INDIRECT` in `zeo-rt`): set = frame push/pop/set_line
/// must go through capi (trace events, pending-label handover).
pub const GATE_FRAMES_INDIRECT_BIT: u16 = 512;

/// The `zeo_rt_gates` bits that stand a TYPED DIRECT CALL down whatever class
/// it nominated: `GATE_PENDING` (2), `GATE_MOVED` (4), `GATE_ARITY_DEBUG` (8),
/// `GATE_MRO_DUPLICATES` (256), `GATE_FRAMES_INDIRECT` (512) and
/// `GATE_PENDING_EXTENDS` (1024).
///
/// The four narrowed latches are deliberately absent -- `GATE_PATCHED_ANY`,
/// `GATE_ANY_SINGLETONS`, `GATE_ANCESTRY_MUTATED`, `GATE_ANY_EXTENDED` -- and
/// [`PATCHED_BITS_SYM`]'s per-class bit is what replaces them. Testing the
/// whole word for zero cost a program 54% on a loop of typed calls for one
/// `define_method` on an unrelated class.
///
/// `zeo-rt` owns the bit numbering and asserts this mask against it at
/// compile time.
pub const GATE_TYPED_DIRECT_SLOW: u16 = 2 | 4 | 8 | 256 | 512 | 1024;

/// The exported bitmap of classes whose method resolution may differ from the
/// frozen registry -- one bit per class id, word `id / 64`, bit `id % 64`.
/// Emitted code loads its word at a link-time-known address.
pub const PATCHED_BITS_SYM: &str = "zeo_rt_patched_bits";

/// How many class ids [`PATCHED_BITS_SYM`] covers. A nominated class at or
/// above this takes the dispatch route; `zeo-rt` answers for it from the set.
pub const PATCHED_BITS_IDS: u32 = 1 << 16;

// --- The C-side program description -----------------------------------------
//
// Everything below crosses the boundary as `.rodata` tables pointed to by one
// `ProgramDesc`; `zeo-rt`'s `register_program` walks them in the generated
// `main` order. String pairs are UTF-8 bytes living for the process (the
// program's own read-only data), which is what lets the runtime borrow them
// as `&'static str`. Every struct is plain `#[repr(C)]` data -- no behavior
// on this side of the contract.

/// Bumped when `ProgramDesc` (or any row struct) changes shape;
/// `register_program` refuses a mismatch loudly.
pub const ABI_VERSION: u32 = 1;

/// One 24-byte Ruby value slot, opaque on this side -- `zeo-rt`'s
/// `abi_layout` test asserts it is exactly `RubyValue`'s size and alignment,
/// which is what makes the fn-pointer types below interchangeable with the
/// runtime's own (by assertion, not nominal identity).
#[repr(C, align(8))]
pub struct Value {
    _bytes: [u8; VALUE_SIZE],
}

/// A `(ptr, len)` string pair. `len == 0` may carry a null `ptr` (an absent
/// optional string).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Str {
    pub ptr: *const u8,
    pub len: usize,
}

/// A compiled method/trampoline body -- the mirror of `zeo-rt`'s
/// `capi::ValueFn`. `recv`/`argv` are borrowed; `blk` is MOVED in (null =
/// no block; the callee consumes it); `out` receives the value on
/// [`STATUS_OK`], and [`STATUS_SIGNAL`] parks the `Signal` in the pending
/// slot.
pub type ValueFn = unsafe extern "C" fn(
    recv: *const Value,
    argv: *const Value,
    argc: usize,
    blk: *mut Value,
    out: *mut Value,
) -> i32;

/// A compiled top level or feature unit: no receiver, just the status
/// protocol.
pub type UnitFn = unsafe extern "C" fn(out: *mut Value) -> i32;

/// How [`ClassDesc::kind`] selects the registrar. Kinds beyond `Plain` and
/// `Module` are declared for the emitter's benefit; `register_program`
/// panics on the ones no emitter produces yet.
pub const CLASS_PLAIN: u8 = 0;
pub const CLASS_MODULE: u8 = 1;
pub const CLASS_EXCEPTION: u8 = 2;
pub const CLASS_VALUE_SUBCLASS: u8 = 3;
pub const CLASS_RECV_HONOURING: u8 = 4;
pub const CLASS_MODULE_SUBCLASS: u8 = 5;
pub const CLASS_WEAK_MAP: u8 = 6;
pub const CLASS_IMMEDIATE: u8 = 7;

/// One class/module the program defines. `ancestors` is the fully
/// linearized chain (self first), exactly what today's generated `register`
/// call carries; `ivar_names` are the declared slot names in slot order,
/// `hidden` the trailing `Struct`/`Data` member count.
#[repr(C)]
pub struct ClassDesc {
    pub id: u32,
    pub name: Str,
    pub kind: u8,
    pub ancestors: *const u32,
    pub n_ancestors: usize,
    pub ivar_names: *const Str,
    pub n_ivars: usize,
    /// A compiled `Struct`/`Data`'s MEMBER names, in declaration order --
    /// what `register_compiled_struct` hands `Struct`'s one shared
    /// protocol so `to_a`/`[]`/`==`/`each`/`dig`/`inspect`/Marshal reach
    /// the members by index. Empty for every other class.
    pub members: *const Str,
    pub n_members: usize,
    pub hidden: u16,
}

/// An object-channel row (a compiled class's own `def` -- the CLIF
/// `ruby_class! dispatch{}` twin).
#[repr(C)]
pub struct ObjRow {
    pub class: u32,
    pub name: Str,
    pub f: ValueFn,
}

/// [`VmRow::flags`] bits.
pub const VM_SHADOW: u32 = 1;
pub const VM_INTERNAL: u32 = 2;
pub const VM_CFUNC: u32 = 4;

/// A value-channel row (reopened builtin / top-level `def`) -- the
/// `__VM_ROWS` twin.
#[repr(C)]
pub struct VmRow {
    pub class: u32,
    pub box_id: u32,
    pub name: Str,
    pub f: ValueFn,
    pub flags: u32,
}

/// Marks a [`VmRow`] as inherited from an ancestor rather than defined on
/// `class` itself -- the `__VM_FOREIGN` twin.
#[repr(C)]
pub struct ForeignRow {
    pub class: u32,
    pub name: Str,
}

/// A class-method row (`def self.x`) -- the `__CM_ROWS` twin.
#[repr(C)]
pub struct CmRow {
    pub class: u32,
    pub name: Str,
    pub f: ValueFn,
}

/// A visibility stamp, applied in row order. Verbs are the runtime's
/// `mark_visibility_rows` scheme: 0 private, 1 protected, 2 public,
/// 3 private_class_method, 4 public_class_method.
#[repr(C)]
pub struct VisRow {
    pub class: u32,
    pub name: Str,
    pub verb: u8,
}

/// [`ParamC::kind`] values, in `zeo-rt`'s `ParamKind` declaration order.
pub const PARAM_REQ: u8 = 0;
pub const PARAM_OPT: u8 = 1;
pub const PARAM_REST: u8 = 2;
pub const PARAM_KEYREQ: u8 = 3;
pub const PARAM_KEY: u8 = 4;
pub const PARAM_KEYREST: u8 = 5;
pub const PARAM_BLOCK: u8 = 6;

/// One parameter of a [`MetaRowC`]. `name.len == 0` = unnamed (a bare `*`).
#[repr(C)]
pub struct ParamC {
    pub kind: u8,
    pub name: Str,
}

/// One block literal's PROC SHAPE: every compile-time constant a proc
/// creation used to hand over as nine separate call arguments (plus a
/// stack-built [`ParamC`] array per creation), baked once into the
/// program's `zeo_proc_shapes` table. `params` points at the shape's own
/// rows inside the same table.
#[repr(C)]
pub struct ProcShapeC {
    /// The block's arity (`Proc#arity`).
    pub arity: i32,
    /// The runtime's proc flag bits (lambda, wants-home).
    pub flags: u32,
    /// `#source_location`'s line.
    pub line: u32,
    pub n_params: u32,
    /// The shape's [`ParamC`] rows (null when `n_params` is 0).
    pub params: *const ParamC,
    /// `#source_location`'s file ('' = no location).
    pub file: Str,
    /// The Ractor outer-capture verdict ('' = isolable).
    pub outer: Str,
    /// The frame label ruby gives this block -- `block in <main>`,
    /// `block (2 levels) in Foo#m`. A compile-time fact (the nesting is
    /// lexical), so it is stamped here rather than derived at run time;
    /// `RubyVM::InstructionSequence#label` reads it back, and `#base_label`
    /// strips the `block ... in ` prefix.
    pub label: Str,
}
/// [`ProcShapeC`]'s size -- what the emitter offsets the table by.
pub const PROC_SHAPE_SIZE: usize = 72;
const _: () = assert!(size_of::<ProcShapeC>() == PROC_SHAPE_SIZE);

/// `ParamDescC.rest`/`.kwrest` kinds: no `*` at all, an anonymous `*`
/// (collects and DISCARDS -- no signature slot), or `*name` (one slot
/// holding the runtime-built Array/Hash).
pub const PARAM_STAR_NONE: u8 = 0;
pub const PARAM_STAR_ANON: u8 = 1;
pub const PARAM_STAR_NAMED: u8 = 2;

/// One keyword parameter of a [`ParamDescC`], in DECLARED order (required
/// and optional interleave exactly as written -- the slot order).
#[repr(C)]
pub struct KwParamC {
    pub name: Str,
    /// 1 = required (`k:`); 0 = optional (`k: default` -- the default runs
    /// in the body when the slot is absent).
    pub required: u8,
}

/// A compiled method's parameter shape: what `zeo_rt_bind_params` routes a
/// dynamic call's argv into (the trampoline path). Signature slots, in
/// order: required, optional, rest (when NAMED), post, keywords (declared
/// order), kwrest (when NAMED) -- the same order the direct entry's
/// parameters take. `name` feeds the binder's error text; `file`/`label`/
/// `line`/`end_line` are the CALLEE frame its raises run under (CRuby
/// attributes argument errors to the def line). All `Str`s are `.rodata`.
#[repr(C)]
pub struct ParamDescC {
    pub nreq: u32,
    pub nopt: u32,
    pub npost: u32,
    pub rest: u8,
    pub kwrest: u8,
    /// `**nil` -- refuses keywords BEFORE the arity check.
    pub no_keywords: u8,
    /// The rest came from a TRAILING COMMA (`|a,|`): lenient binding like
    /// an anonymous `*`, but a lambda's arity stays strict and
    /// `Proc#arity` ignores it (block shapes only; methods refuse it).
    pub implicit_rest: u8,
    pub kws: *const KwParamC,
    pub n_kws: usize,
    pub name: Str,
    pub file: Str,
    pub label: Str,
    pub line: u32,
    pub end_line: u32,
}

/// One method's reflection facts -- the `MetaRow` twin. `file.len == 0` =
/// no source location; `aliased_from.len == 0` = not an alias.
#[repr(C)]
pub struct MetaRowC {
    pub class: u32,
    pub singleton: u8,
    pub name: Str,
    pub params: *const ParamC,
    pub n_params: usize,
    pub file: Str,
    pub line: u32,
    pub aliased_from: Str,
}

/// One registrar call today's generated `main` makes that is not a method
/// row (includes/extends, aliases, undefs, super-target bridges, boot
/// redefs, singleton surrogates, ...). Each `kind` number lands with the
/// emitter arm that produces it; `register_program` refuses an unknown
/// kind loudly.
///
/// Kinds so far:
/// - [`REG_MARK_OWN_CLASS_METHOD_ROWS`]: `a` = the method name a
///   `def self.x` WROTE on `class` (reflection's `Method#owner` truth,
///   as opposed to the materialized copies descendants dispatch through).
/// - [`REG_MARK_OWN_ROWS`]: the instance-method twin -- `a` = a name
///   `class`'s own body wrote.
#[repr(C)]
pub struct RegRow {
    pub kind: u8,
    pub class: u32,
    pub a: Str,
    pub b: Str,
    pub f: Option<ValueFn>,
    pub ids: *const u32,
    pub n_ids: usize,
    pub flag: u8,
}

/// `RegRow.kind`: mark `a` as a class-method name `class` itself wrote.
pub const REG_MARK_OWN_CLASS_METHOD_ROWS: u8 = 1;

/// `RegRow.kind`: mark `a` as an instance-method name `class` itself
/// wrote (vs a copy materialization gave it) -- `instance_methods(false)`
/// and `Method#owner` truth.
pub const REG_MARK_OWN_ROWS: u8 = 2;

/// `RegRow.kind`: register `f` as `class`'s OWN `super`-target row for
/// method `a` -- the per-position contribution `send_super_from`'s MRO
/// walk (and `Method#super_method`) consults. CLIF trampolines are
/// receiver-generic `ValueFn`s, so the ordinary trampoline serves.
pub const REG_SUPER_TARGET_VALUE: u8 = 3;

/// `RegRow.kind`: `class` extends the modules in `ids` (`class C; extend M`
/// or `module M; extend self`) -- they join C's SINGLETON chain, which the
/// linearized `ancestors` deliberately excludes, so class-method dispatch,
/// `C.is_a?(M)` and `C.singleton_class.ancestors` walk them separately.
pub const REG_EXTENDS: u8 = 4;

/// `RegRow.kind`: one `extend`ed-module method copy (winner or shadowed) as
/// a SINGLETON-chain super target on `class`, keyed `(ids[0] = module, a)` --
/// what `call_singleton_super_target` consults. `f` carries the trampoline.
pub const REG_SINGLETON_SUPER_TARGET: u8 = 5;

/// `RegRow.kind`: `a` on `class` is an attr-GENERATED accessor over ivar
/// slot `ids[0]` (`flag` = 1 for the writer half). The value CallSite's
/// fill consults this to cache the slot itself instead of the accessor
/// trampoline -- registration data, never dispatch truth: the method
/// table still carries the trampoline.
pub const REG_ACCESSOR_SLOT: u8 = 18;

/// `RegRow.kind`: class method `a` on `class` is a MATERIALIZED copy of an
/// `extend`ed module's row, retired until the `extend` statement seats the
/// module. CRuby has no such copy -- `rb_extend_object` puts the module in
/// the singleton chain where the statement stands -- so the name must not
/// answer, and a hook it supplies must not fire, above the `extend`.
pub const REG_DEFER_EXTENDED_CLASS_METHOD: u8 = 19;

/// `RegRow.kind`: a NAME-indirection alias whose source is a builtin
/// method (`a` = new name, `b` = old/terminal name) -- the send miss paths
/// rewrite through it; `validate_class_aliases` raises `NameError` for a
/// source that resolves nowhere, at the class body's run.
pub const REG_ALIAS: u8 = 6;

/// [`REG_ALIAS`]'s singleton-side twin (`class << self; alias [] new`):
/// the row lands in the table a class-OBJECT receiver consults.
pub const REG_CLASS_ALIAS: u8 = 7;

/// `RegRow.kind`: `undef`'s mark -- `a` on `class` answers "not defined"
/// from this position down, shadowing any inherited row.
pub const REG_MARK_UNDEFINED: u8 = 8;

/// `RegRow.kind`: `class` is runtime-CONDITIONAL -- its shape registers so
/// the static MRO has one, but the constant stays concealed until the
/// guarded body reveals it.
pub const REG_CONCEAL_CLASS: u8 = 9;

/// A builtin reopen that CHANGED the class's ancestry (`class Array;
/// include M; end`): `ids` = the full ancestor chain, patched over the
/// entry `register_builtins` already made rather than replacing it (a
/// re-register would carry no constructor).
pub const REG_SET_ANCESTORS: u8 = 10;

/// A require-GATED builtin (an extension class) registers per program --
/// `register_builtins` covers only the always-on ones. `a` = the
/// fully-qualified display name, `flag` = is-a-module, `ids` = the
/// ancestor chain; the entry carries no constructor, exactly as rustc's
/// `__registry.register(.., None)` does.
pub const REG_REGISTER_BUILTIN: u8 = 11;

/// A compile-registered SINGLETON-class surrogate -- the module a
/// constant-bearing or mixin-bearing `class << self` body is homed on.
/// `class` = the surrogate, `ids[0]` = its owner; seeding the runtime mint
/// is what makes `Owner.singleton_class` answer it (and so what puts a
/// `singleton_class.prepend`ed module in its `ancestors`).
pub const REG_SINGLETON_SURROGATE: u8 = 12;

/// The BOOT install of the first body of a method with an observable
/// redefinition timeline: `class` = the owner, `a` = the method name, `f`
/// = that body's trampoline, `ids[0]` = its row in [`ProgramDesc::redef_metas`].
/// It runs before the first statement, so the window before each reopen's
/// positional re-install dispatches -- and reflects -- the way ruby's
/// install-where-it-stands does.
pub const REG_BOOT_REDEF: u8 = 13;

/// One `private_constant` name: `class` = the owner, `a` = the name. The
/// reference guard asks the runtime flag at the site (a later
/// `public_constant` restores the name), and `Module#constants` and
/// `defined?` read it too.
pub const REG_CONST_PRIVATE: u8 = 14;

/// A definition hook written on `Module`/`Class`/`BasicObject` ITSELF, which
/// answers for every class in the program: `a` = the hook name. No per-class
/// owner scan can see one -- the reopen registers an ordinary instance
/// method whose owner is the very class the no-op default lives on -- so the
/// compiler records it by name and the runtime consults the list.
pub const REG_MARK_GLOBAL_DEF_HOOK: u8 = 15;

/// `zeo_rt_reflect_dispatch_in`'s `entry`: the reflection method as
/// WRITTEN at the site -- which is both what the MRO is asked about and
/// what an ordinary call dispatches when the answer is "shadowed".
pub const REFLECT_SEND: u8 = 0;
/// See [`REFLECT_SEND`].
pub const REFLECT_PUBLIC_SEND: u8 = 1;
/// See [`REFLECT_SEND`].
pub const REFLECT_RESPOND_TO: u8 = 2;
/// See [`REFLECT_SEND`].
pub const REFLECT_METHOD: u8 = 3;

/// A `refine` HOLDER class: `class` = the holder, `ids[0]` = the refining
/// module, `ids[1]` = the refined target. The holder is a module in every
/// respect but one -- its own `.class` is `Refinement`, which is what a
/// refined `Method#owner` reports.
pub const REG_MARK_REFINEMENT: u8 = 16;

/// A class or module a `Ruby::Box` owns: `class` = the class,
/// `ids[0]` = the box's internal id. A box's top-level classes are
/// registered under their bare ruby names -- `Escapee` inside a box is
/// still called `Escapee` -- so without this mark the registry's name
/// table would hand main a class the box wrote. Emitted only for a class
/// whose box is not 0, so a program with no box registers no rows and the
/// runtime's side table stays empty.
pub const REG_MARK_BOX_CLASS: u8 = 17;

/// The byte size of the emitter's opaque `for`-loop state slot -- one
/// stack slot per loop, filled by `zeo_rt_for_begin` and released by
/// `zeo_rt_for_end`. The runtime asserts its own struct fits.
pub const FOR_STATE_SIZE: usize = 96;

/// A lazily-run feature unit -- the `install_feature_units` twin.
#[repr(C)]
pub struct UnitRow {
    pub feature: Str,
    pub f: UnitFn,
}

/// A feature on the compiled-in load path zeo could not lower -- the
/// `install_declined_features` twin.
#[repr(C)]
pub struct DeclinedRow {
    pub feature: Str,
    pub reason: Str,
}

/// One source file's coverable lines -- the `coverage_install` row twin.
#[repr(C)]
pub struct CovFile {
    pub file: Str,
    pub total: u32,
    pub stmt_lines: *const u32,
    pub n_stmt: usize,
    pub def_lines: *const u32,
    pub n_def: usize,
}

/// The one table of tables an emitted program hands `zeo_rt_main`.
/// Registration order inside each table -- and the table walk order -- is
/// today's generated `main` order. (The rustc backend's `__FILES`/`__SYMS`
/// pools have no row here: CLIF programs reference `.rodata` strings
/// directly and `unit_init` interns its own symbol table.)
/// One embedded source file: the load-path-relative spelling it answers to,
/// and its text.
#[repr(C)]
pub struct SourceRow {
    pub path: Str,
    pub text: Str,
}

#[repr(C)]
pub struct ProgramDesc {
    pub abi_version: u32,
    pub classes: *const ClassDesc,
    pub n_classes: usize,
    pub obj_rows: *const ObjRow,
    pub n_obj_rows: usize,
    pub vm_rows: *const VmRow,
    pub n_vm_rows: usize,
    pub vm_foreign: *const ForeignRow,
    pub n_vm_foreign: usize,
    pub cm_rows: *const CmRow,
    pub n_cm_rows: usize,
    pub vis_rows: *const VisRow,
    pub n_vis_rows: usize,
    pub meta_rows: *const MetaRowC,
    pub n_meta_rows: usize,
    /// One reflection row per BODY of a method with an observable
    /// redefinition timeline, in the order `analyze::redefs` compiled them.
    /// They are NOT registered at boot: the row for the position that is
    /// live installs itself, from [`REG_BOOT_REDEF`] for the first body and
    /// from the positional install for each later one.
    pub redef_metas: *const MetaRowC,
    pub n_redef_metas: usize,
    /// The builtin class method tables this program can reach, as pointers to
    /// the `zeo_ctable_*` symbols `ruby_class!` exports.
    ///
    /// A program NAMES its tables because a link-time collection cannot be
    /// dropped: a `linkme` slice entry is a `no_dead_strip` root in its own
    /// right, so every builtin method body was unstrippable and a `puts 1`
    /// binary carried Date's parser, Marshal and all of OpenSSL. Naming them
    /// here lets the linker keep exactly what the program can use.
    pub class_tables: *const *const core::ffi::c_void,
    pub n_class_tables: usize,
    pub reg_rows: *const RegRow,
    pub n_reg_rows: usize,
    pub units: *const UnitRow,
    pub n_units: usize,
    pub declined: *const DeclinedRow,
    pub n_declined: usize,
    pub coverage: *const CovFile,
    pub n_cov: usize,
    pub loaded_features: *const Str,
    pub n_loaded: usize,
    pub load_path: *const Str,
    pub n_load_path: usize,
    /// How many leading `load_path` entries a RUN-TIME `require` may search.
    /// The `-I` roots come first and are searchable; the roots of the gems
    /// zeo itself compiled in follow and are not -- their Ruby half is
    /// already linked, so loading it again from disk would build a second,
    /// half-native copy. `$LOAD_PATH` still names them, because that is what
    /// CRuby shows and what code reading `$:` for a real file needs.
    pub n_load_path_search: usize,
    pub parse_warnings: *const Str,
    pub n_warnings: usize,
    /// The `--embed-sources` pack: ruby source the RUN TIME may need for a
    /// require the compiler could not resolve. Empty unless the flag was
    /// given -- a hermetic binary is the default, and embedding every
    /// source would silently double the artifact.
    pub sources: *const SourceRow,
    pub n_sources: usize,
    /// `__END__`'s carrier file (`len == 0` = none) and byte offset.
    pub data_section: Str,
    pub data_offset: u64,
    /// The `<main>` body: pushes its own frame, `check_ints`, validates
    /// aliases, runs class bodies then the program statements.
    pub toplevel: UnitFn,
    /// Interns the symbol table and initializes the `.bss` site slots.
    pub unit_init: Option<unsafe extern "C" fn()>,
    /// `zeo_eval_install` when the program can eval; else `None`.
    pub eval_install: Option<unsafe extern "C" fn()>,
}

// ---------------------------------------------------------------------------
// FFI call descriptors
//
// An `attach_function` wrapper body is one C call whose whole shape --
// argument types, return type, `blocking:`, `:varargs` -- is compile-time
// knowledge. The Cranelift backend bakes it into `.rodata` as the tree
// below and hands the runtime ONE pointer, so the emitted code lowers only
// the argument EXPRESSIONS: an argument list half-built when a coercion
// raises can then strand nothing.
// ---------------------------------------------------------------------------

/// [`FfiTypeC::tag`]: an ordinary C scalar, named by `scalar`.
pub const FFI_TY_SCALAR: u8 = 0;
/// A named `enum`: `members` maps Symbol to `int` in both directions.
pub const FFI_TY_ENUM: u8 = 1;
/// An `enum` whose members only the running program knows -- `slot` names
/// the runtime store the class body filled.
pub const FFI_TY_ENUM_SLOT: u8 = 2;
/// A `callback`: `sub` holds the C argument types, `scalar` the C return.
pub const FFI_TY_CALLBACK: u8 = 3;
/// A struct passed or returned BY VALUE: `sub` holds the field types in
/// declaration order (inline arrays already expanded) and `size` the byte
/// width.
pub const FFI_TY_STRUCT: u8 = 4;
/// `:strptr` -- a `char *` return read back as `[String, Pointer]`.
pub const FFI_TY_STRPTR: u8 = 5;

/// One `enum` member, in declaration order.
#[repr(C)]
pub struct FfiEnumMemberC {
    pub name: Str,
    pub value: i64,
}

/// One C type in a call descriptor. Which fields carry meaning is decided
/// by `tag`; the rest are zero.
#[repr(C)]
pub struct FfiTypeC {
    pub tag: u8,
    /// The `CScalar` code (`zeo_abi::ffi::CScalar::code`) -- the type
    /// itself for [`FFI_TY_SCALAR`], the RETURN type for a callback.
    pub scalar: u8,
    pub slot: usize,
    pub members: *const FfiEnumMemberC,
    pub n_members: usize,
    pub sub: *const FfiTypeC,
    pub n_sub: usize,
    pub size: usize,
}

/// One `attach_function` call site's whole C signature.
#[repr(C)]
pub struct FfiCallC {
    pub args: *const FfiTypeC,
    pub n_args: usize,
    pub ret: FfiTypeC,
    /// `blocking: true` -- run the call with the GVL released.
    pub blocking: u8,
    /// `:varargs` -- the LAST value in `argv` is the wrapper's `*rest`, a
    /// flat Array of alternating `(type Symbol, value)` pairs.
    pub variadic: u8,
}

/// [`FfiSymMode`] as a byte: resolve the symbol in the named libraries only.
pub const FFI_SYM_LIB: u8 = 0;
/// Resolve in the named libraries, falling back to the process image -- the
/// tier a build-time `#[link(name = ..)]` served for the rustc backend,
/// where the library is linked into the program and its symbols are simply
/// present.
pub const FFI_SYM_LIB_OR_PROCESS: u8 = 1;
/// Resolve in the process image only (no `ffi_lib`, or `FFI::CURRENT_PROCESS`).
pub const FFI_SYM_PROCESS: u8 = 2;
