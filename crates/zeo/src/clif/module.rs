//! The emission module and its state: `ClifModule` (an object file for
//! AOT, in-process code memory for JIT -- same lowering either way) and
//! `Emitter` (the rodata blob, the symbol pool, and the capi import
//! cache).

use super::capi_names::{self, CTy};
use crate::diagnostics::clif::{CResult, CodegenError};
use cranelift_codegen::ir::{self, AbiParam, types};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{
    DataDescription, DataId, FuncId, Linkage, Module, ModuleDeclarations, ModuleReloc, ModuleResult,
};
use cranelift_object::{ObjectBuilder, ObjectModule};
use std::collections::HashMap;

/// The one module the emitter writes into. Both arms take the identical
/// lowering -- the enum (not two emitters) is what makes "JIT runs the
/// same code AOT links" a structural fact.
#[expect(
    clippy::large_enum_variant,
    reason = "one ClifModule exists per compile; boxing would buy nothing"
)]
pub(crate) enum ClifModule {
    Object(ObjectModule),
    Jit(JITModule),
}

impl ClifModule {
    fn as_dyn(&self) -> &dyn Module {
        match self {
            ClifModule::Object(m) => m,
            ClifModule::Jit(m) => m,
        }
    }

    fn as_dyn_mut(&mut self) -> &mut dyn Module {
        match self {
            ClifModule::Object(m) => m,
            ClifModule::Jit(m) => m,
        }
    }
}

impl Module for ClifModule {
    fn isa(&self) -> &dyn cranelift_codegen::isa::TargetIsa {
        self.as_dyn().isa()
    }

    fn declarations(&self) -> &ModuleDeclarations {
        self.as_dyn().declarations()
    }

    fn declare_function(
        &mut self,
        name: &str,
        linkage: Linkage,
        signature: &ir::Signature,
    ) -> ModuleResult<FuncId> {
        self.as_dyn_mut().declare_function(name, linkage, signature)
    }

    fn declare_anonymous_function(&mut self, signature: &ir::Signature) -> ModuleResult<FuncId> {
        self.as_dyn_mut().declare_anonymous_function(signature)
    }

    fn declare_data(
        &mut self,
        name: &str,
        linkage: Linkage,
        writable: bool,
        tls: bool,
    ) -> ModuleResult<DataId> {
        self.as_dyn_mut().declare_data(name, linkage, writable, tls)
    }

    fn declare_anonymous_data(&mut self, writable: bool, tls: bool) -> ModuleResult<DataId> {
        self.as_dyn_mut().declare_anonymous_data(writable, tls)
    }

    fn define_function_with_control_plane(
        &mut self,
        func: FuncId,
        ctx: &mut cranelift_codegen::Context,
        ctrl_plane: &mut cranelift_codegen::control::ControlPlane,
    ) -> ModuleResult<()> {
        self.as_dyn_mut()
            .define_function_with_control_plane(func, ctx, ctrl_plane)
    }

    fn define_function_bytes(
        &mut self,
        func_id: FuncId,
        alignment: u64,
        bytes: &[u8],
        relocs: &[ModuleReloc],
    ) -> ModuleResult<()> {
        self.as_dyn_mut()
            .define_function_bytes(func_id, alignment, bytes, relocs)
    }

    fn define_data(&mut self, data_id: DataId, data: &DataDescription) -> ModuleResult<()> {
        self.as_dyn_mut().define_data(data_id, data)
    }
}

/// How many functions queue up before the backend runs over them. Big enough
/// to keep every core busy, small enough that the queue's own memory stays
/// beside the noise: a program requiring rubygems lowers about 207,000
/// functions, and holding them all would cost more than the compile.
const PARALLEL_BATCH: usize = 2048;

/// How many threads the backend runs on. `ZEO_CODEGEN_THREADS` overrides it,
/// which is what makes the parallel and the one-core path comparable on the
/// same binary; `1` turns the queue off entirely.
fn codegen_threads() -> usize {
    static N: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *N.get_or_init(|| {
        std::env::var("ZEO_CODEGEN_THREADS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|n| *n > 0)
            .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get()))
    })
}

/// One lowered function waiting for the backend.
struct PendingFn {
    id: FuncId,
    func: ir::Function,
    label: String,
    debug_rows: bool,
}

/// One compiled function waiting to be defined into the module. The context
/// travels with it because both halves of the serial step read it: the
/// relocations name its function, and DWARF reads its machine buffer.
struct PendingCode {
    id: FuncId,
    label: String,
    debug_rows: bool,
    /// The buffer's alignment, or why the function did not compile. Reported
    /// on the serial side so the failure names one function rather than
    /// arriving from a thread.
    alignment: Result<u64, String>,
    ctx: cranelift_codegen::Context,
}

/// Program-wide emission state: the module, the rodata blob, the symbol
/// How a compile-time class id becomes a machine value.
///
/// `Immediate` is today's whole-program compile: every id is an `iconst`.
/// `Packaged` is what makes a package object POSITION-INDEPENDENT: an id at
/// or past `first` (the shared bootstrap's end -- the object's own band)
/// reads from the id-translation table the HOST fills at link time, so the
/// same object is correct in any program, wherever its band lands. Builtin
/// ids below `first`, and the `u32::MAX` sentinel, stay immediates in both
/// modes. The `packaged-ids` debug flag forces this mode program-wide with
/// an identity table, which is the bench upper bound for the load cost.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum IdMode {
    Immediate,
    Packaged { first: u32 },
}

/// Slots in a package's `{prefix}_bases` stride array: one u32 per
/// program-dense site space, filled by the host at merge time. A package
/// mints each space's ids dense from zero and its emitted code adds the
/// loaded stride; the host bakes its own ids past every package's total.
pub(crate) const BASE_UNIT: u32 = 0;
pub(crate) const BASE_REGEXP: u32 = 1;
pub(crate) const BASE_FLIPFLOP: u32 = 2;
pub(crate) const BASE_REDEF: u32 = 3;
pub(crate) const N_BASES: usize = 4;

/// pool, and the capi import cache.
pub(crate) struct Emitter {
    pub module: ClifModule,
    pub ptr: ir::Type,
    pub rodata_id: DataId,
    pub syms_id: DataId,
    pub callsites_id: DataId,
    /// One entry per emitted inline-cache slot: the caller class the site
    /// is vetted against (`u32::MAX` = ruby's FCALL, no visibility
    /// question). The index IS the slot index.
    pub callsites: Vec<u32>,
    pub cm_sites_id: DataId,
    /// How many class-method cache slots the program needs; the index IS
    /// the slot index, and a slot carries no per-site constant.
    pub cm_sites: usize,
    pub ffi_sites_id: DataId,
    /// How many `attach_function` address words this unit holds. Counted
    /// apart from `ffi_sites`, which is the RUNTIME's site id and comes from
    /// the runtime's own band in an `eval`.
    pub ffi_words: u32,
    pub const_sites_id: DataId,
    /// How many constant-read cache slots the program needs; same
    /// index-is-the-slot rule as `cm_sites`.
    pub const_sites: usize,
    pub new_sites_id: DataId,
    /// How many compiled-construction cache slots the program needs; same
    /// index-is-the-slot rule as `cm_sites`.
    pub new_sites: usize,
    pub dyn_sites_id: DataId,
    /// How many dynamic-caller cache slots the program needs; same
    /// index-is-the-slot rule as `cm_sites`.
    pub dyn_sites: usize,
    pub proc_shapes_id: DataId,
    /// The proc shapes collected during lowering (`emit_proc_new`), laid
    /// out interleaved -- each shape's `ParamC` rows follow it, so a
    /// shape's byte offset is final the moment it is pushed.
    pub proc_shapes: Vec<super::statics::ProcShapeSpec>,
    /// The running byte length of `zeo_proc_shapes`.
    pub proc_shapes_len: usize,
    /// The runtime's `zeo_rt_pending_interrupts` counter (an import, not
    /// a program-local table) -- what `Fx::check_ints` loads inline.
    pub pending_id: DataId,
    /// The runtime's `zeo_rt_gates` word: an emitter fast path loads it
    /// inline and masks it.
    pub gates_id: DataId,
    /// The runtime's `zeo_rt_patched_bits` bitmap -- one bit per class id.
    /// A typed direct call loads its own class's word inline, at an address
    /// and with a mask both fixed at compile time.
    pub patched_bits_id: DataId,
    pub reopen_flags_id: DataId,
    /// `(builtin class id, method name)` -> its byte in `zeo_reopen_flags`.
    /// See [`super::names::REOPEN_FLAGS`]; empty for a program that reopens no
    /// builtin, which is almost all of them.
    pub reopen_flags: HashMap<(u32, String), u32>,
    /// The package build this emission is, when it is
    /// one -- names the unit machinery `{prefix}_unit_*`, exports the
    /// row-referenced bodies, and swaps the desc for a manifest.
    pub pkg: Option<crate::packages::package::PackageBuild>,
    /// How a class id becomes a machine value -- see
    /// [`IdMode`] and `Fx::cid_value`.
    pub id_mode: IdMode,
    /// The id-translation table's declaration, made once on first use:
    /// `{prefix}_cids` imported by a package (the HOST defines it with the
    /// band it assigned), `zeo_cids` defined locally by the
    /// `packaged-ids` bench mode.
    pub cids_id: Option<DataId>,
    /// The per-package stride array, same shape: `{prefix}_bases`,
    /// imported by a package, defined by the host with the strides it
    /// assigned -- one u32 per program-dense site space ([`BASE_UNIT`],
    /// [`BASE_REGEXP`], [`BASE_FLIPFLOP`]).
    pub bases_id: Option<DataId>,
    /// The first reveal-group id THIS compile may use.
    /// Merged packages own `[0, unit_base)`; every unit index and
    /// alias-reveal group this program bakes -- the reveal calls and the
    /// `REG_CONCEAL_METHOD` rows both -- is offset by it. Zero when no
    /// packages merge, which keeps today's output byte-identical.
    pub unit_base: u32,
    /// The first flip-flop latch id THIS compile may use, on
    /// `unit_base`'s rule: merged packages own `[0, flip_flop_base)`.
    pub flip_flop_base: u32,
    /// The first redef-meta row THIS compile may use, on the same rule;
    /// the merged packages' rows sit at the front of the one table.
    pub redef_base: u32,
    pub syms: super::statics::SymPool,
    rodata: Vec<u8>,
    rodata_offsets: HashMap<Vec<u8>, u32>,
    imports: HashMap<&'static str, FuncId>,
    /// Compiled methods by Ruby name -- what a receiverless call resolves
    /// against for the direct path.
    pub methods: HashMap<String, MethodDecl>,
    /// Compiled INSTANCE-method bodies by `(class id, ruby name)` -- what
    /// a typed direct call resolves against. Materialization emits an
    /// inherited body per class, so a subclass receiver's copy is its own
    /// row. Plain main-box user-class bodies only: accessor rows fold
    /// through `zeo_rt_attr_read`, and builtin reopens, module rows and
    /// boxed classes keep the dispatch path.
    pub typed_methods: HashMap<(u32, String), MethodDecl>,
    /// Class-body sites by their INLINE `ClassDef` marker: what a marker
    /// in statement position emits (a hoisted site's marker is absent --
    /// its body already ran in the toplevel prelude).
    pub class_bodies: HashMap<crate::hir::NodeId, super::collect::ClassBodyCall>,
    /// `(class, scope)` -> the trampoline that installs that body, for the
    /// boot install and each positional `MethodRedefine`.
    pub redef_tramps: HashMap<(u32, u32), FuncId>,
    /// Which `reopen_flags` indices belong to a LAZY UNIT's reopen -- see
    /// `collect_reopen_flags`. Their guard falls through to the body when
    /// the builtin has no native row of that name.
    pub unit_reopen_flags: std::collections::HashSet<u32>,
    /// The same key's row in `ProgramDesc::redef_metas` -- what the install
    /// at this body's document position registers so reflection answers the
    /// body that is live rather than the last one written.
    pub redef_metas: HashMap<(u32, u32), u32>,
    fn_index: u32,
    /// How long Cranelift's own backend has taken, and over how many
    /// functions -- see [`Emitter::define`], the one door every emitted
    /// function goes through. `--log-level info` reports both at the end of
    /// a compile, which is what separates the cost of the backend from the
    /// cost of the lowering that feeds it.
    pub codegen_nanos: u64,
    pub codegen_fns: u32,
    /// Functions lowered but not yet handed to Cranelift, when the backend
    /// may run on more than one core. Flushed in batches rather than at the
    /// end: a program that requires rubygems lowers about 207,000 functions,
    /// and holding every one of them would cost more memory than the whole
    /// compile.
    pending: Vec<PendingFn>,
    /// Whether the backend may run on more than one core -- see
    /// [`Emitter::define`].
    parallel: bool,
    /// Regexp-literal site ids -- one cached frozen object per site
    /// (`zeo_rt_regexp_lit`).
    pub regexp_sites: u32,
    /// `attach_function` call-site ids -- one resolved C symbol address
    /// per site in the runtime.
    pub ffi_sites: u32,
    /// Whether this unit is a run-time `eval`. Both caches above live in the
    /// RUNNING process, so a snippet's fresh counter would answer the
    /// program's cached object; a snippet mints from the runtime instead.
    pub eval_sites: bool,
    /// Line coverage: whether the program activated it, and the statement
    /// lines stamped so far (the `def` lines it never stamps come from
    /// `analyze::coverage::def_lines`). Empty in a program without the
    /// `require` -- the instrumentation is absent, not gated.
    pub cov_active: bool,
    pub cov_lines: std::collections::BTreeMap<String, std::collections::BTreeSet<u32>>,
    /// When `Some`, every finished function's CLIF renders here (before
    /// machine compilation -- the target-independent IR), with every
    /// function and symbol under its linkage NAME rather than its module
    /// index, so `zeo backend` can read the text back.
    pub clif_text: Option<String>,
    /// The `.zeodata` twin of `clif_text`, drafted by `emit_program` once
    /// the row tables exist: the sidecar, or why the program has none.
    pub sidecar: Option<Result<crate::backend::sidecar::Sidecar, String>>,
    /// When `Some`, statements stamp a `SourceLoc` and the object carries
    /// DWARF built from them (`-g`).
    pub debug: Option<super::debuginfo::DebugInfo>,
}

/// One compiled method's declaration facts.
pub(crate) struct MethodDecl {
    pub body: FuncId,
    pub tramp: FuncId,
    /// Required-positional count (the direct-call gate compares it).
    pub arity: usize,
    /// Required positionals only -- a count-matched call site may go
    /// direct; any richer shape routes through the dynamic send and the
    /// bound trampoline.
    pub plain: bool,
    /// Callee keyword names in slot order when EVERY keyword is required
    /// and nothing else complicates the signature -- see `Layout`.
    pub kw_direct: Option<Vec<String>>,
    pub has_blk: bool,
    /// Whether this row REPLACES a builtin body and so carries a positional
    /// reopen guard. The guard lives in the TRAMPOLINE, so a direct call
    /// would jump straight past it -- and the row would answer from program
    /// start, which is the thing the guard exists to prevent.
    pub reopen_flagged: bool,
    /// Whether a compiled-in UNIT wrote this `def`, so its row is CONCEALED
    /// until the unit's file runs. Same reason as `reopen_flagged`: the
    /// concealment is a registry question the dynamic send asks, and a
    /// direct call never asks it -- so `leaked_helper(1, 2)` answered from
    /// program start for a file nothing had required.
    pub concealed: bool,
}

impl Emitter {
    /// `jit`: emit into in-process code memory instead of an object file.
    /// The only lowering-visible difference is `is_pic` (`JITModule`
    /// requires non-PIC code); everything downstream is mode-blind.
    pub(crate) fn new(jit: bool) -> CResult<Emitter> {
        let mut flags = settings::builder();
        let set = |flags: &mut settings::Builder, k: &str, v: &str| {
            flags
                .set(k, v)
                .map_err(|e| CodegenError::internal(format!("cranelift setting {k}={v}: {e}")))
        };
        // `speed_and_size` was measured and is a NULL RESULT on both axes:
        // fib/gcbench/nested_loop/loops_times all within noise, and hello
        // 15,303,448 against 15,303,464 bytes -- an emitted program's size
        // is `libzeo.a`, not its own code.
        set(&mut flags, "opt_level", "speed")?;
        set(&mut flags, "is_pic", if jit { "false" } else { "true" })?;
        set(&mut flags, "preserve_frame_pointers", "true")?;
        set(&mut flags, "enable_probestack", "false")?;
        set(&mut flags, "unwind_info", "true")?;
        set(&mut flags, "libcall_call_conv", "isa_default")?;
        let verify = cfg!(debug_assertions) || std::env::var_os("ZEO_CLIF_VERIFY").is_some();
        set(
            &mut flags,
            "enable_verifier",
            if verify { "true" } else { "false" },
        )?;
        // An object file is a SHIPPED artifact, so it takes the triple's
        // baseline and infers nothing. `cranelift_native::builder()` reads the
        // BUILD machine's CPU -- AVX/AVX2/FMA/BMI1/BMI2/LZCNT on x86,
        // lse/pauth/fp16/dotprod on aarch64 -- and a binary built with them
        // executes an illegal instruction on an older chip. The JIT may
        // legitimately infer: its code runs in this process, on this CPU.
        let isa = cranelift_native::builder_with_options(jit)
            .map_err(|e| {
                CodegenError::internal(format!("cranelift has no backend for this host: {e}"))
            })?
            .finish(settings::Flags::new(flags))
            .map_err(|e| CodegenError::internal(format!("building the target ISA: {e}")))?;
        if isa.triple().endianness() != Ok(target_lexicon::Endianness::Little) {
            return Err(CodegenError::internal(
                "the clif backend only serializes little-endian tables",
            ));
        }
        // Only an ELF object carries `.eh_frame`, and only the context-taking
        // `define_function` writes it -- so ELF is the one configuration whose
        // backend must stay on one core. See [`Emitter::define`].
        let elf = isa.triple().binary_format == target_lexicon::BinaryFormat::Elf;
        let parallel = jit || !elf;
        let mut module = if jit {
            // Imports resolve against the runtime linked into THIS process:
            // the capi table first (`zeo_rt_*` -- not exported, so dlsym
            // cannot find them), then cranelift's dlsym fallback (libcalls:
            // memcpy and friends from libc).
            let mut builder = JITBuilder::with_isa(isa, cranelift_module::default_libcall_names());
            builder.symbols(zeo_rt::capi::symbols::NAMES.iter().map(|&name| {
                let addr = zeo_rt::capi::symbols::addr(name)
                    .expect("every listed capi symbol has an address");
                (name, addr)
            }));
            builder.symbols(zeo_rt::capi::symbols::DATA_NAMES.iter().map(|&name| {
                let addr = zeo_rt::capi::symbols::data_addr(name)
                    .expect("every listed capi data symbol has an address");
                (name, addr)
            }));
            ClifModule::Jit(JITModule::new(builder))
        } else {
            let mut builder =
                ObjectBuilder::new(isa, "zeo-p0", cranelift_module::default_libcall_names())
                    .map_err(|e| {
                        CodegenError::internal(format!("cranelift object builder: {e}"))
                    })?;
            builder.per_function_section(true);
            // `.eh_frame` is free on ELF; Mach-O emission panics in
            // cranelift-object 0.134 and is not load-bearing.
            builder.unwind_info(elf);
            ClifModule::Object(ObjectModule::new(builder))
        };
        let ptr = module.target_config().pointer_type();
        let rodata_id = module
            .declare_data(super::names::RODATA, Linkage::Local, false, false)
            .map_err(|e| {
                CodegenError::internal(format!("declaring {}: {e}", super::names::RODATA))
            })?;
        let syms_id = module
            .declare_data(super::names::SYMS, Linkage::Local, true, false)
            .map_err(|e| {
                CodegenError::internal(format!("declaring {}: {e}", super::names::SYMS))
            })?;
        let callsites_id = module
            .declare_data(super::names::CALLSITES, Linkage::Local, true, false)
            .map_err(|e| {
                CodegenError::internal(format!("declaring {}: {e}", super::names::CALLSITES))
            })?;
        let cm_sites_id = module
            .declare_data(super::names::CM_SITES, Linkage::Local, true, false)
            .map_err(|e| {
                CodegenError::internal(format!("declaring {}: {e}", super::names::CM_SITES))
            })?;
        let ffi_sites_id = module
            .declare_data(super::names::FFI_SITES, Linkage::Local, true, false)
            .map_err(|e| {
                CodegenError::internal(format!("declaring {}: {e}", super::names::FFI_SITES))
            })?;
        let const_sites_id = module
            .declare_data(super::names::CONST_SITES, Linkage::Local, true, false)
            .map_err(|e| {
                CodegenError::internal(format!("declaring {}: {e}", super::names::CONST_SITES))
            })?;
        let new_sites_id = module
            .declare_data(super::names::NEW_SITES, Linkage::Local, true, false)
            .map_err(|e| {
                CodegenError::internal(format!("declaring {}: {e}", super::names::NEW_SITES))
            })?;
        let dyn_sites_id = module
            .declare_data(super::names::DYN_SITES, Linkage::Local, true, false)
            .map_err(|e| {
                CodegenError::internal(format!("declaring {}: {e}", super::names::DYN_SITES))
            })?;
        let proc_shapes_id = module
            .declare_data(super::names::PROC_SHAPES, Linkage::Local, false, false)
            .map_err(|e| {
                CodegenError::internal(format!("declaring {}: {e}", super::names::PROC_SHAPES))
            })?;
        let reopen_flags_id = module
            .declare_data(super::names::REOPEN_FLAGS, Linkage::Local, true, false)
            .map_err(|e| {
                CodegenError::internal(format!("declaring {}: {e}", super::names::REOPEN_FLAGS))
            })?;
        // The runtime's pending-interrupt counter, read inline at loop
        // checkpoints. Import linkage: the archive defines it on the AOT
        // path, `symbols::data_addr` on the JIT path.
        let pending_id = module
            .declare_data("zeo_rt_pending_interrupts", Linkage::Import, true, false)
            .map_err(|e| {
                CodegenError::internal(format!("declaring zeo_rt_pending_interrupts: {e}"))
            })?;
        let gates_id = module
            .declare_data("zeo_rt_gates", Linkage::Import, true, false)
            .map_err(|e| CodegenError::internal(format!("declaring zeo_rt_gates: {e}")))?;
        let patched_bits_id = module
            .declare_data(zeo_abi::abi::PATCHED_BITS_SYM, Linkage::Import, true, false)
            .map_err(|e| CodegenError::internal(format!("declaring zeo_rt_patched_bits: {e}")))?;
        Ok(Emitter {
            patched_bits_id,
            module,
            ptr,
            rodata_id,
            syms_id,
            callsites_id,
            ffi_sites_id,
            ffi_words: 0,
            reopen_flags_id,
            reopen_flags: HashMap::new(),
            pkg: None,
            id_mode: IdMode::Immediate,
            cids_id: None,
            bases_id: None,
            unit_base: 0,
            flip_flop_base: 0,
            redef_base: 0,
            callsites: Vec::new(),
            cm_sites_id,
            cm_sites: 0,
            const_sites_id,
            const_sites: 0,
            new_sites_id,
            new_sites: 0,
            dyn_sites_id,
            dyn_sites: 0,
            proc_shapes_id,
            proc_shapes: Vec::new(),
            proc_shapes_len: 0,
            pending_id,
            gates_id,
            syms: super::statics::SymPool::default(),
            rodata: Vec::new(),
            rodata_offsets: HashMap::new(),
            imports: HashMap::new(),
            methods: HashMap::new(),
            typed_methods: HashMap::new(),
            class_bodies: HashMap::new(),
            redef_tramps: HashMap::new(),
            unit_reopen_flags: std::collections::HashSet::new(),
            redef_metas: HashMap::new(),
            fn_index: 0,
            codegen_nanos: 0,
            codegen_fns: 0,
            pending: Vec::new(),
            parallel: parallel && codegen_threads() > 1,
            regexp_sites: 0,
            ffi_sites: 0,
            eval_sites: false,
            cov_active: false,
            cov_lines: std::collections::BTreeMap::new(),
            clif_text: None,
            sidecar: None,
            debug: None,
        })
    }

    /// The next regexp-literal site id: the runtime's own space for a
    /// snippet, this unit's dense counter otherwise.
    pub(crate) fn mint_regexp_site(&mut self) -> u32 {
        if self.eval_sites {
            return zeo_rt::capi::literals::reserve_regexp_sites(1);
        }
        self.regexp_sites += 1;
        self.regexp_sites - 1
    }

    /// [`Emitter::mint_regexp_site`]'s twin for an `attach_function` site.
    pub(crate) fn mint_ffi_site(&mut self) -> u32 {
        if self.eval_sites {
            return zeo_rt::ffi::reserve_sym_sites(1);
        }
        self.ffi_sites += 1;
        self.ffi_sites - 1
    }

    /// The index of a fresh `zeo_ffi_sites` slot -- this unit's own word
    /// for one `attach_function` site's resolved address.
    pub(crate) fn mint_ffi_word(&mut self) -> u32 {
        self.ffi_words += 1;
        self.ffi_words - 1
    }

    /// The id-translation table's `DataId`, declared on first use. A
    /// PACKAGE imports it (the host defines it with the assigned band);
    /// the `packaged-ids` bench mode declares it locally and
    /// `define_identity_cids` fills it.
    pub(crate) fn cids_data_id(&mut self) -> DataId {
        if let Some(id) = self.cids_id {
            return id;
        }
        let (name, linkage) = match &self.pkg {
            Some(p) => (format!("{}_cids", p.prefix()), Linkage::Import),
            None => ("zeo_cids".to_string(), Linkage::Local),
        };
        let id = self
            .module
            .declare_data(&name, linkage, false, false)
            .expect("declaring the id-translation table");
        self.cids_id = Some(id);
        id
    }

    /// The stride array's `DataId`, same declare-once shape as
    /// [`Emitter::cids_data_id`]. Only a package has one.
    pub(crate) fn bases_data_id(&mut self) -> DataId {
        if let Some(id) = self.bases_id {
            return id;
        }
        let pkg = self.pkg.as_ref().expect("only a package loads a stride");
        let name = format!("{}_bases", pkg.prefix());
        let id = self
            .module
            .declare_data(&name, Linkage::Import, false, false)
            .expect("declaring the stride array");
        self.bases_id = Some(id);
        id
    }

    /// A compiled body/trampoline symbol, package-prefixed when this
    /// emission is a package: `zeo_t_X_y` becomes `zeo_pkg_<name>_t_X_y`.
    /// Every such symbol is EXPORTED from a package object (the host's
    /// merged desc names it), so the prefix is what keeps two objects
    /// defining the same Ruby class/method from colliding at link.
    pub(crate) fn pkg_symbol(&self, base: String) -> String {
        match &self.pkg {
            Some(p) => format!(
                "{}_{}",
                p.prefix(),
                base.strip_prefix("zeo_").unwrap_or(&base)
            ),
            None => base,
        }
    }

    /// `bytes`' offset in the rodata blob (deduplicated).
    pub(crate) fn intern_rodata(&mut self, bytes: &[u8]) -> u32 {
        if let Some(&off) = self.rodata_offsets.get(bytes) {
            return off;
        }
        let off = u32::try_from(self.rodata.len()).expect("rodata under 4GB");
        self.rodata.extend_from_slice(bytes);
        self.rodata_offsets.insert(bytes.to_vec(), off);
        off
    }

    /// Like `intern_rodata`, but the offset is `align`-aligned (id arrays
    /// the runtime reads as `&[u32]`). Not deduplicated -- alignment is
    /// part of the identity and these tables are tiny.
    pub(crate) fn intern_rodata_aligned(&mut self, bytes: &[u8], align: usize) -> u32 {
        let pad = (align - (self.rodata.len() % align)) % align;
        self.rodata.extend(std::iter::repeat_n(0u8, pad));
        let off = u32::try_from(self.rodata.len()).expect("rodata under 4GB");
        self.rodata.extend_from_slice(bytes);
        off
    }

    pub(crate) fn take_rodata(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.rodata)
    }

    pub(crate) fn rodata(&self) -> &[u8] {
        &self.rodata
    }

    /// Start the blob with bytes whose offsets code written elsewhere
    /// already indexes (`zeo backend`). Later interning only appends, so
    /// every offset that code carries stays true.
    pub(crate) fn seed_rodata(&mut self, bytes: Vec<u8>) {
        debug_assert!(self.rodata.is_empty(), "seeded after interning began");
        self.rodata = bytes;
    }

    pub(crate) fn syms_len(&self) -> u32 {
        self.syms_names().len() as u32
    }

    pub(crate) fn syms_names(&self) -> &[String] {
        self.syms.names()
    }

    fn ctype(&self, t: CTy) -> ir::Type {
        match t {
            CTy::Ptr | CTy::Usize => self.ptr,
            CTy::I32 | CTy::U32 => types::I32,
            CTy::U8 | CTy::I8 => types::I8,
            CTy::F64 => types::F64,
        }
    }

    fn abi_param(&self, t: CTy) -> AbiParam {
        match t {
            // A sub-32-bit C ARGUMENT is the caller's to zero-extend (the
            // Apple arm64 rule; a no-op elsewhere).
            CTy::U8 => AbiParam::new(types::I8).uext(),
            CTy::I8 => AbiParam::new(types::I8).sext(),
            CTy::Ptr | CTy::Usize | CTy::I32 | CTy::U32 | CTy::F64 => AbiParam::new(self.ctype(t)),
        }
    }

    /// Record `func`'s CLIF when `--emit-clif` asked for it: `label` as a
    /// comment, then the function under its symbol, with every callee and
    /// data symbol named. Cranelift's own printer writes module indices
    /// (`u0:3`, `userextname2`), which mean nothing outside this module;
    /// the names are what `zeo backend` resolves.
    pub(crate) fn record_clif(&mut self, id: FuncId, label: &str, func: &ir::Function) {
        if self.clif_text.is_none() {
            return;
        }
        let rendered = self.symbolic_clif(id, func);
        let text = self.clif_text.as_mut().expect("checked above");
        text.push_str(&format!(";; {label}\n{rendered}\n"));
    }

    fn symbolic_clif(&self, id: FuncId, func: &ir::Function) -> String {
        use cranelift_codegen::entity::EntityRef;
        let decls = self.module.declarations();
        let symbol = |ext: &ir::UserExternalName| -> String {
            match ext.namespace {
                0 => {
                    let f = FuncId::from_u32(ext.index);
                    format!("%{}", decls.get_function_decl(f).linkage_name(f))
                }
                _ => {
                    let d = DataId::from_u32(ext.index);
                    format!("%{}", decls.get_data_decl(d).linkage_name(d))
                }
            }
        };
        let named = func.params.user_named_funcs();
        // A callee prints as `u<namespace>:<index>`, a data symbol as
        // `userextname<ref>`; both resolve through the function's own
        // name table.
        let resolve = |token: &str| -> Option<String> {
            if let Some(n) = token.strip_prefix("userextname") {
                let r = ir::UserExternalNameRef::new(n.parse().ok()?);
                return named.get(r).map(symbol);
            }
            let (ns, index) = token.strip_prefix('u')?.split_once(':')?;
            let want = ir::UserExternalName::new(ns.parse().ok()?, index.parse().ok()?);
            named.values().find(|n| **n == want).map(symbol)
        };
        let mut out = String::new();
        for (i, line) in func.display().to_string().lines().enumerate() {
            if i == 0 {
                let rest = line.split_once('(').map_or("", |(_, rest)| rest);
                out.push_str(&format!(
                    "function %{}({rest}\n",
                    decls.get_function_decl(id).linkage_name(id)
                ));
            } else if line.starts_with("    fn") || line.starts_with("    gv") {
                let words: Vec<String> = line
                    .split_whitespace()
                    .map(|w| resolve(w).unwrap_or_else(|| w.to_string()))
                    .collect();
                out.push_str("    ");
                out.push_str(&words.join(" "));
                out.push('\n');
            } else {
                out.push_str(line);
                out.push('\n');
            }
        }
        out
    }

    /// Take one just-compiled function's line rows, the `record_clif`
    /// twin for DWARF. Must run while the context still holds the
    /// `CompiledCode` -- i.e. right after `define_function`.
    pub(crate) fn record_debug(
        &mut self,
        name: &str,
        func: FuncId,
        ctx: &cranelift_codegen::Context,
    ) {
        if let Some(debug) = &mut self.debug
            && let Some(code) = ctx.compiled_code()
        {
            debug.record(func, name, code);
        }
    }

    /// Hand one finished function to Cranelift's backend.
    ///
    /// Every emitted function goes through here -- bodies, class bodies,
    /// blocks, trampolines, accessors, units, `main`. One door is what makes
    /// the backend's cost measurable against the lowering above it, and it is
    /// the single place the parallel backend intercepts.
    ///
    /// Lowering builds the CLIF against shared emitter state (the rodata
    /// blob, the symbol pool, the site counters), so it stays on one thread.
    /// Turning that CLIF into machine code needs nothing but the function and
    /// the target, and it is 92% of a compile -- so it queues here and runs on
    /// every core, in batches.
    ///
    /// `debug_rows` asks for the function's DWARF line rows. Only a real Ruby
    /// body carries them; a trampoline is generated code with no source line
    /// of its own.
    pub(crate) fn define(
        &mut self,
        id: FuncId,
        func: ir::Function,
        label: &str,
        debug_rows: bool,
    ) -> CResult<()> {
        if !self.parallel {
            return self.define_now(id, func, label, debug_rows);
        }
        self.pending.push(PendingFn {
            id,
            func,
            label: label.to_string(),
            debug_rows,
        });
        if self.pending.len() >= PARALLEL_BATCH {
            self.flush_pending()?;
        }
        Ok(())
    }

    /// [`Emitter::define`] on this thread, which is what a configuration
    /// carrying unwind information must use: only the context-taking
    /// `define_function` writes `.eh_frame`.
    fn define_now(
        &mut self,
        id: FuncId,
        func: ir::Function,
        label: &str,
        debug_rows: bool,
    ) -> CResult<()> {
        let mut ctx = self.module.make_context();
        ctx.func = func;
        let started = std::time::Instant::now();
        let outcome = self.module.define_function(id, &mut ctx);
        self.codegen_nanos += started.elapsed().as_nanos() as u64;
        self.codegen_fns += 1;
        outcome.map_err(|e| {
            let inner = match e {
                cranelift_module::ModuleError::Compilation(inner) => inner,
                other => return CodegenError::internal(format!("compiling {label}: {other}")),
            };
            CodegenError::internal(format!("compiling {label}: {}", codegen_failure(&inner)))
        })?;
        if debug_rows {
            self.record_debug(label, id, &ctx);
        }
        Ok(())
    }

    /// Compile every queued function, then define them IN THE ORDER THEY WERE
    /// QUEUED.
    ///
    /// The ordering is not a detail: `per_function_section` gives each
    /// function its own section, so defining in completion order would lay the
    /// object out differently on every run and no two builds of one program
    /// would match.
    pub(crate) fn flush_pending(&mut self) -> CResult<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let batch = std::mem::take(&mut self.pending);
        let count = batch.len() as u32;
        let started = std::time::Instant::now();

        // Compiled functions, paired with the position they were queued at.
        // The isa borrow ends with this block, which is what lets the serial
        // half below take `&mut self` again.
        let mut compiled: Vec<(usize, PendingCode)> = {
            let isa = self.module.isa();
            let queue = std::sync::Mutex::new(batch.into_iter().enumerate());
            let done = std::sync::Mutex::new(Vec::with_capacity(count as usize));
            std::thread::scope(|scope| {
                for _ in 0..codegen_threads() {
                    scope.spawn(|| {
                        // One control plane per worker: it is a deterministic
                        // fuzzing dial, and the default makes no choices.
                        let mut ctrl = cranelift_codegen::control::ControlPlane::default();
                        loop {
                            let next = queue.lock().expect("the work queue is not poisoned").next();
                            let Some((at, item)) = next else { break };
                            let mut ctx = cranelift_codegen::Context::new();
                            ctx.func = item.func;
                            let alignment = ctx
                                .compile(isa, &mut ctrl)
                                .map(|code| code.buffer.alignment as u64)
                                .map_err(|e| {
                                    format!(
                                        "compiling {}: {}",
                                        item.label,
                                        codegen_failure(&e.inner)
                                    )
                                });
                            done.lock().expect("the result list is not poisoned").push((
                                at,
                                PendingCode {
                                    id: item.id,
                                    label: item.label,
                                    debug_rows: item.debug_rows,
                                    alignment,
                                    ctx,
                                },
                            ));
                        }
                    });
                }
            });
            done.into_inner().expect("every worker has finished")
        };
        compiled.sort_by_key(|(at, _)| *at);

        for (_, one) in compiled {
            let alignment = one.alignment.map_err(CodegenError::internal)?;
            let code = one
                .ctx
                .compiled_code()
                .expect("a function that compiled has code");
            let relocs: Vec<ModuleReloc> = code
                .buffer
                .relocs()
                .iter()
                .map(|reloc| ModuleReloc::from_mach_reloc(reloc, &one.ctx.func, one.id))
                .collect();
            self.module
                .define_function_bytes(one.id, alignment, code.buffer.data(), &relocs)
                .map_err(|e| CodegenError::internal(format!("defining {}: {e}", one.label)))?;
            if one.debug_rows {
                self.record_debug(&one.label, one.id, &one.ctx);
            }
        }
        self.codegen_nanos += started.elapsed().as_nanos() as u64;
        self.codegen_fns += count;
        Ok(())
    }

    /// A fresh `UserFuncName` index (cosmetic; must be unique per module).
    pub(crate) fn next_fn_index(&mut self) -> u32 {
        self.fn_index += 1;
        self.fn_index
    }

    /// The Cranelift signature of one runtime entry point.
    pub(crate) fn capi_signature(&self, row: &capi_names::CapiSig) -> ir::Signature {
        let mut sig = self.module.make_signature();
        sig.params
            .extend(row.params.iter().map(|&t| self.abi_param(t)));
        if let Some(ret) = row.ret {
            // Returns carry no extension claim -- only the low bits are
            // read, at the value's own width.
            sig.returns.push(AbiParam::new(self.ctype(ret)));
        }
        sig
    }

    /// The import `FuncId` for capi symbol `name` (declared once).
    pub(crate) fn import(&mut self, name: &'static str) -> FuncId {
        if let Some(&id) = self.imports.get(name) {
            return id;
        }
        let sig = self.capi_signature(capi_names::sig(name));
        let id = self
            .module
            .declare_function(name, Linkage::Import, &sig)
            .unwrap_or_else(|e| panic!("declaring capi import {name}: {e}"));
        self.imports.insert(name, id);
        id
    }
}

/// What a cranelift failure says. `CodegenError`'s own `Display` answers a
/// bare "Verifier errors" for the one kind that carries detail, and the
/// detail -- which instruction, which value, which block -- is the whole
/// message.
fn codegen_failure(err: &cranelift_codegen::CodegenError) -> String {
    match err {
        cranelift_codegen::CodegenError::Verifier(errs) => format!("{errs:#?}"),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use cranelift_module::Module;

    /// Every ISA feature `cranelift_native::infer_native_flags` can turn on
    /// by reading the build machine's CPU. A binary carrying one of these
    /// runs only where that chip does.
    const INFERRED: &[&str] = &[
        "has_sse3",
        "has_ssse3",
        "has_sse41",
        "has_sse42",
        "has_avx",
        "has_avx2",
        "has_fma",
        "has_avx512bitalg",
        "has_avx512dq",
        "has_avx512f",
        "has_avx512vl",
        "has_avx512vbmi",
        "has_bmi1",
        "has_bmi2",
        "has_lzcnt",
        "has_lse",
        "has_pauth",
        "has_fp16",
        "has_dotprod",
        "sign_return_address",
        "sign_return_address_with_bkey",
    ];

    /// `zeo -o` emits for the triple's BASELINE. Inferring the build
    /// machine's features ties the artifact to that microarchitecture and
    /// SIGILLs on an older chip -- the JIT is the only mode allowed to infer,
    /// because its code never leaves the process that built it.
    #[test]
    fn an_object_binary_is_not_tied_to_the_build_machine() {
        let emitter = super::Emitter::new(false).expect("an ISA for this host");
        let on: Vec<String> = emitter
            .module
            .isa()
            .isa_flags()
            .iter()
            .filter(|v| INFERRED.contains(&v.name) && v.as_bool() == Some(true))
            .map(|v| v.name.to_string())
            .collect();
        assert!(
            on.is_empty(),
            "the object path enabled host-inferred CPU features: {on:?}"
        );
    }
}
