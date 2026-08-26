//! The emission module and its state: `ClifModule` (an object file for
//! AOT, in-process code memory for JIT -- same lowering either way) and
//! `Emitter` (the rodata blob, the symbol pool, and the capi import
//! cache).

use super::capi_names::{self, CTy};
use crate::codegen_error::{CResult, CodegenError};
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
#[allow(
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

/// Program-wide emission state: the module, the rodata blob, the symbol
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
    /// inline, and only ZERO takes the guard-free arm.
    pub gates_id: DataId,
    pub reopen_flags_id: DataId,
    /// `(builtin class id, method name)` -> its byte in `zeo_reopen_flags`.
    /// See [`super::names::REOPEN_FLAGS`]; empty for a program that reopens no
    /// builtin, which is almost all of them.
    pub reopen_flags: HashMap<(u32, String), u32>,
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
    /// machine compilation -- the target-independent IR).
    pub clif_text: Option<String>,
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
            let elf = isa.triple().binary_format == target_lexicon::BinaryFormat::Elf;
            let mut builder =
                ObjectBuilder::new(isa, "zeo-p0", cranelift_module::default_libcall_names())
                    .map_err(|e| {
                        CodegenError::internal(format!("cranelift object builder: {e}"))
                    })?;
            builder.per_function_section(true);
            // `.eh_frame` is free on ELF; Mach-O emission panics in
            // cranelift-object 0.134 and is not load-bearing (decision 13).
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
        Ok(Emitter {
            module,
            ptr,
            rodata_id,
            syms_id,
            callsites_id,
            reopen_flags_id,
            reopen_flags: HashMap::new(),
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
            regexp_sites: 0,
            ffi_sites: 0,
            eval_sites: false,
            cov_active: false,
            cov_lines: std::collections::BTreeMap::new(),
            clif_text: None,
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

    /// Record `func`'s CLIF when `--emit-clif` asked for it, under its
    /// exported symbol name.
    pub(crate) fn record_clif(&mut self, name: &str, func: &ir::Function) {
        if let Some(text) = &mut self.clif_text {
            use std::fmt::Write as _;
            let _ = writeln!(text, ";; {name}\n{}", func.display());
        }
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

    /// A fresh `UserFuncName` index (cosmetic; must be unique per module).
    pub(crate) fn next_fn_index(&mut self) -> u32 {
        self.fn_index += 1;
        self.fn_index
    }

    /// The import `FuncId` for capi symbol `name` (declared once).
    pub(crate) fn import(&mut self, name: &'static str) -> FuncId {
        if let Some(&id) = self.imports.get(name) {
            return id;
        }
        let row = capi_names::sig(name);
        let mut sig = self.module.make_signature();
        sig.params
            .extend(row.params.iter().map(|&t| self.abi_param(t)));
        if let Some(ret) = row.ret {
            // Returns carry no extension claim -- only the low bits are
            // read, at the value's own width.
            sig.returns.push(AbiParam::new(self.ctype(ret)));
        }
        let id = self
            .module
            .declare_function(name, Linkage::Import, &sig)
            .unwrap_or_else(|e| panic!("declaring capi import {name}: {e}"));
        self.imports.insert(name, id);
        id
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
