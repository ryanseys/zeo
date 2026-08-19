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
/// Rust-or-C two-word implementation, M0-4).
pub const CALLSITE_SIZE: usize = 56;
pub const DYNCALLER_SITE_SIZE: usize = 56;
pub const CLASSMETHOD_SITE_SIZE: usize = 48;
pub const CONST_SITE_SIZE: usize = 40;
pub const CIVAR_SITE_SIZE: usize = 40;
pub const REGEXP_SITE_SIZE: usize = 16;
pub const FFISYM_SITE_SIZE: usize = 16;
/// The common alignment of every site struct above.
pub const SITE_ALIGN: usize = 8;

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

/// [`VmRow::flags`] bits. Zero until the corelib mechanism (G8) emits them.
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
    pub _pad: u8,
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
/// redefs, singleton surrogates, ...). The `kind` numbering lands with the
/// emitter arms that produce each row (M1); until then `register_program`
/// refuses any row loudly.
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
    pub parse_warnings: *const Str,
    pub n_warnings: usize,
    /// `__END__`'s carrier file (`len == 0` = none) and byte offset.
    pub data_section: Str,
    pub data_offset: u64,
    /// The `<main>` body: pushes its own frame, `check_ints`, validates
    /// aliases, runs class bodies then the program statements.
    pub toplevel: UnitFn,
    /// Interns the symbol table and initializes the `.bss` site slots.
    pub unit_init: Option<unsafe extern "C" fn()>,
    /// `zeo_eval_install` when the program can eval (G6); else `None`.
    pub eval_install: Option<unsafe extern "C" fn()>,
}
