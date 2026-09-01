//! User-class compilation: the plain-class slice (`class Node ... end`) --
//! `ClassDesc` collection, method/accessor rows on the OBJECT channel, and
//! the eligibility rules that refuse everything the M0 slice cannot carry
//! (modules, mixins, class methods, runtime class bodies, non-Object
//! superclass machinery).

use super::module::Emitter;
use super::{names, params, statics};
use crate::analyze::Analyzed;
use crate::codegen_error::{CResult, CodegenError};
use crate::compiler::AccessorKind;
use cranelift_module::{Linkage, Module};
use zeo_abi::ClassId;

/// One compiled user class.
pub(crate) struct ClassSpec {
    pub id: u32,
    pub name: String,
    pub ancestors: Vec<u32>,
    pub ivars: Vec<String>,
    pub hidden: u16,
    /// A compiled `Struct`/`Data`'s member names, in declaration order
    /// (`register_compiled_struct`); empty for every other class.
    pub members: Vec<String>,
    /// `module M` -- registered `CLASS_MODULE` (no allocator, no layout);
    /// its methods ride the VALUE channel instead of the object channel.
    /// A `zeo_abi::abi::CLASS_*` registrar selector.
    pub kind: u8,
}

/// One object-channel method to compile for a user class.
pub(crate) struct ObjMethodSpec {
    pub owner: ClassId,
    pub owner_name: String,
    pub name: String,
    pub body: Vec<crate::hir::NodeId>,
    pub node: Option<crate::hir::NodeId>,
    pub tramp: cranelift_module::FuncId,
    /// `Some` = an accessor devirtualized to a slot trampoline (no body fn
    /// at all -- the trampoline IS the method); `None` = an ordinary body +
    /// trampoline pair. The flag is `attr_generated`: a GENERATED accessor
    /// is iseq-less in CRuby, so it carries no frame, while one folded from
    /// a hand-written `def` still does.
    pub accessor: Option<(usize, AccessorKind, bool)>,
    pub body_fn: Option<cranelift_module::FuncId>,
    pub hir_params: crate::hir::Params,
    /// The name this method was ALIASED from -- reflection's
    /// `Method#original_name`; `None` for an ordinary `def`.
    pub alias_of: Option<String>,
    pub has_blk: bool,
    pub ruby2_keywords: bool,
    pub dyn_ivars: bool,
    /// The class the `def` was WRITTEN in (a module method keeps the
    /// module) -- where its `super` resumes.
    pub defining_class: ClassId,
    /// The singleton-class SURROGATE the `def` was lexically written in (a
    /// constant-bearing `class << self` body). Lexical questions -- bare
    /// constants, `Module.nesting` -- resolve through it. See
    /// `Scope::lexical_home`.
    pub lexical_home: Option<ClassId>,
    /// This entry is the class's OWN write (not a materialized ancestor
    /// copy): its trampoline doubles as the own-`super`-target row.
    pub is_own: bool,
    /// An own method SHADOWED by a `prepend` winner: its body compiles and
    /// its `REG_SUPER_TARGET_VALUE` row registers, but no `ObjRow` -- the
    /// object channel carries the module's materialized copy.
    pub super_target_only: bool,
    /// This row REUSES a body and trampoline that another row already emits:
    /// a definition on Object/Kernel/BasicObject, which `collect::
    /// collect_methods` emits once under `Object#name` for the whole program.
    /// The registration row is real; the emission is somebody else's, so
    /// `emit` must not compile a second copy or define the trampoline twice.
    pub shared: bool,
}

/// One class method (`def self.x`) to compile -- a `CmRow` on the
/// class-method channel. The body ALWAYS receives the runtime receiver as
/// `self`, so
/// a subclass inheriting the method runs under its own `self`.
pub(crate) struct CmMethodSpec {
    /// `false` = a singleton-super-target-only body (a shadowed `extend`
    /// copy): compiled and registered under `(module, name)`, but no
    /// `CmRow` on the class-method channel.
    pub cm_row: bool,
    /// The box this `def self.x` was WRITTEN in; 0 is main. A box reopening
    /// a shared class keeps its class methods to itself, the way its
    /// instance methods already do.
    pub box_id: u32,
    /// The class the `def` was WRITTEN in -- the class itself for a
    /// `def self.x`, the MODULE for an `extend`ed copy. A class-method
    /// `super` resumes the singleton chain after it.
    pub defining_class: ClassId,
    /// The singleton-class SURROGATE the `def` was lexically written in (a
    /// constant-bearing `class << self` body). Lexical questions -- bare
    /// constants, `Module.nesting` -- resolve through it. See
    /// `Scope::lexical_home`.
    pub lexical_home: Option<ClassId>,
    pub owner: ClassId,
    pub owner_name: String,
    pub name: String,
    pub body: Vec<crate::hir::NodeId>,
    pub node: Option<crate::hir::NodeId>,
    pub tramp: cranelift_module::FuncId,
    pub body_fn: cranelift_module::FuncId,
    pub hir_params: crate::hir::Params,
    /// The name this method was ALIASED from -- reflection's
    /// `Method#original_name`; `None` for an ordinary `def`.
    pub alias_of: Option<String>,
    pub has_blk: bool,
    pub ruby2_keywords: bool,
}

/// One SUPERSEDED (or re-installed) body of a method with an observable
/// redefinition timeline. Compiled as a receiver-generic value-channel
/// body -- an overlay entry propagates down the ancestry, so a subclass
/// instance may arrive -- and installed by `runtime_replace_method` at
/// the definition's document position.
pub(crate) struct RedefSpec {
    pub owner: ClassId,
    pub owner_name: String,
    pub scope: crate::compiler::ScopeId,
    pub name: String,
    pub body: Vec<crate::hir::NodeId>,
    pub node: Option<crate::hir::NodeId>,
    pub tramp: cranelift_module::FuncId,
    pub body_fn: cranelift_module::FuncId,
    pub hir_params: crate::hir::Params,
    pub has_blk: bool,
    pub ruby2_keywords: bool,
    /// `def self.x` rather than `def x`. The body's `self` is the class, and
    /// the install writes the class-method side of the overlay.
    pub singleton: bool,
}

/// One module method: body + `ValueFn` trampoline, registered as a
/// `VmRow` on the module's id. The body's `self` is whatever receiver
/// dispatch hands over (a value pointer, as every body here takes).
pub(crate) struct ModMethodSpec {
    pub owner: ClassId,
    /// The `Ruby::Box` this row belongs to. A per-box OVERLAY of a builtin
    /// (`box.eval("class String; def shout; end; end")`) registers on the
    /// ROOT builtin's entry keyed by its box, so only code running in that
    /// box reaches it.
    pub box_id: u32,
    pub owner_name: String,
    pub name: String,
    pub body: Vec<crate::hir::NodeId>,
    pub node: Option<crate::hir::NodeId>,
    pub tramp: cranelift_module::FuncId,
    pub body_fn: cranelift_module::FuncId,
    pub hir_params: crate::hir::Params,
    /// The name this method was ALIASED from -- reflection's
    /// `Method#original_name`; `None` for an ordinary `def`.
    pub alias_of: Option<String>,
    pub has_blk: bool,
    pub ruby2_keywords: bool,
    pub dyn_ivars: bool,
    /// The class the `def` was WRITTEN in (a module method keeps the
    /// module) -- where its `super` resumes.
    pub defining_class: ClassId,
    /// The singleton-class SURROGATE the `def` was lexically written in (a
    /// constant-bearing `class << self` body). Lexical questions -- bare
    /// constants, `Module.nesting` -- resolve through it. See
    /// `Scope::lexical_home`.
    pub lexical_home: Option<ClassId>,
    /// This row REUSES the body and trampoline the DEFINING module already
    /// emits. The registration is real; the emission is somebody else's, so
    /// `emit` must not compile a second copy or define the trampoline twice.
    pub shared: bool,
}

/// What `collect_classes` hands back: the class table plus its method and
/// visibility rows.
pub(crate) struct CollectedClasses {
    pub classes: Vec<ClassSpec>,
    pub methods: Vec<ObjMethodSpec>,
    /// A module's OWN methods as VALUE-channel rows on the module
    /// id. An includer dispatches through its own
    /// materialized object-channel copies first; these rows are what a
    /// dynamic receiver (and the ancestor walk) finds.
    pub module_methods: Vec<ModMethodSpec>,
    pub class_methods: Vec<CmMethodSpec>,
    /// `(class, name)` pairs a `def self.x` WROTE on the class itself --
    /// reflection's `Method#owner` truth (`mark_own_class_method_rows`).
    pub own_cm: Vec<(u32, String)>,
    /// The instance-method twin: names each class's own body wrote
    /// (`mark_own_rows` -- `instance_methods(false)`/`Method#owner`).
    pub own_rows: Vec<(u32, String)>,
    pub vis: Vec<statics::VisRowSpec>,
    /// `(class, module ids)` -- `register_extends` rows: the modules on
    /// each class's SINGLETON chain (`extend M`, `extend self`).
    pub extends: Vec<(u32, Vec<u32>)>,
    /// `(class, name)` -- `undef` marks (`mark_undefined`).
    pub undef_rows: Vec<(u32, String)>,
    /// Runtime-conditional class ids, concealed until their guarded body
    /// reveals them.
    pub conceal: Vec<u32>,
    /// `(surrogate, owner)` for every compile-registered singleton-class
    /// surrogate -- what seeds the runtime's `singleton_class` mint.
    pub singleton_surrogates: Vec<(u32, u32)>,
    /// `(class, name)` -- `private_constant` marks.
    /// Every body of a redefined method, superseded ones included.
    pub redefs: Vec<RedefSpec>,
    /// `(class, name, scope)` -- the FIRST body, installed at boot.
    /// `(class, name, scope, singleton, visibility)`. The visibility is the
    /// scope's own -- a `private :v` between two bodies retagged the FIRST
    /// `def` at lower time, so each body already carries its own mark.
    pub boot_redefs: Vec<(
        u32,
        String,
        crate::compiler::ScopeId,
        bool,
        crate::hir::Visibility,
    )>,
    /// `(class, ancestor ids)` -- a builtin reopen that CHANGED the
    /// ancestry patches the entry `register_builtins` already made.
    pub set_ancestors: Vec<(u32, Vec<u32>)>,
    /// `(class, fq name, is_module, ancestor ids)` -- a require-gated
    /// builtin, which `register_builtins` does not cover.
    pub register_builtin: Vec<(u32, String, bool, Vec<u32>)>,
    /// `(class, new, old, is_class_side)` -- builtin-source alias rows
    /// (`register_alias` / `register_class_alias` name indirections).
    pub alias_rows: Vec<(u32, String, String, bool, u32, bool)>,
    /// `(class, module, name, trampoline)` -- singleton-chain super
    /// targets: every `extend`ed method copy (winner AND shadowed) plus
    /// inherited class methods a subclass's own `def self.x` shadowed
    /// (`define_singleton_super_target`).
    pub sst: Vec<(u32, u32, String, cranelift_module::FuncId)>,
    /// `(class, name)` value rows a builtin reopen INHERITED (a module
    /// method materialized onto the builtin) -- marked foreign so a
    /// `super` walk skips them at that position (`mark_foreign_value_rows`).
    pub foreign: Vec<(u32, String)>,
    /// `(class, name, class_side, unit)` -- rows a compiled-in UNIT wrote,
    /// concealed until that unit runs. See [`conceal_unit_methods`].
    pub conceal_methods: Vec<(u32, String, bool, u32)>,
}

/// The rows a compiled-in unit's `def`s own, which must not answer until
/// that unit's file has run.
///
/// A unit is a load-path file nothing has required yet, and CRuby has no
/// such method until the `require` reaches it. zeo registers the row at
/// startup all the same -- the static MRO needs a shape, and the walk has to
/// find the real body once the unit loads -- so the row is CONCEALED
/// instead, exactly as [`Compiler::class_waits_for_its_unit`] conceals the
/// class's own constant, and the unit's function reveals it.
///
/// Two rules keep the set honest:
///
/// * a class the unit itself DEFINES contributes nothing -- its constant is
///   already concealed, so nothing can name it to reach a method;
/// * a name this class also writes OUTSIDE a unit is left alone. There is
///   one row per name, carrying the last-`def`-wins winner, and concealing
///   it would take the earlier body away too. `analyze::redefs` already owns
///   that timeline.
///
/// Materialization stores the ancestor's own `ScopeId` on the descendant, so
/// a module method a unit wrote is concealed on every class that mixed it
/// in without any extra bookkeeping.
fn conceal_unit_methods(compiler: &crate::compiler::Compiler) -> Vec<(u32, String, bool, u32)> {
    let mut out = Vec::new();
    for (idx, class) in compiler.classes.iter().enumerate() {
        if class.unit.is_some() {
            continue;
        }
        // The row's owner: a per-box overlay registers on the root builtin's
        // entry, which is where the conceal has to land too.
        let owner = class.builtin_overlay.map_or(idx as u32, |root| root.0);
        let written_here: crate::compiler::FSet<(&str, bool)> = class
            .method_history
            .iter()
            .filter(|(_, _, _, sid)| compiler.scope(*sid).unit.is_none())
            .map(|(name, class_side, _, _)| (name.as_str(), *class_side))
            .collect();
        // `methods` is the flattened dispatch set and `class_methods` its
        // class-side twin. `own_methods` is the third, and for a MODULE the
        // only one -- a module's instance methods live on its includers, so
        // its own table stays empty and this list is what names them. A
        // module method a unit wrote reached dispatch through the module's
        // own value row while every includer's copy was already concealed.
        let entries = class
            .methods
            .iter()
            .map(|e| (e.def, false))
            .chain(class.class_methods.iter().map(|e| (e.def, true)))
            .chain(class.own_methods.iter().map(|&sid| (sid, false)));
        for (sid, class_side) in entries {
            let scope = compiler.scope(sid);
            let Some(unit) = scope.unit else {
                continue;
            };
            if written_here.contains(&(scope.name.as_str(), class_side)) {
                continue;
            }
            out.push((owner, scope.name.clone(), class_side, unit));
        }
    }
    // ...and the rows a RUNTIME alias makes positional. Same table, a reveal
    // group of its own, lifted at the `def`'s line rather than a unit's head.
    for (cid, name, class_side, group) in &compiler.alias_source_reveals {
        out.push((cid.0, name.clone(), *class_side, *group));
    }
    out
}

/// Collect + declare every user class and its methods; refusals are loud
/// and name the class.
/// The `extend` edges that are in place from PROGRAM START -- every edge no
/// statement installs.
///
/// A class body's `extend M` and the `base.extend(ClassMethods)` an
/// `included` hook performs are both positional: `rb_extend_object` is
/// `rb_include_module(rb_singleton_class(obj), module)`, which seats the
/// module where the statement stands. Those edges are emitted at their own
/// site instead (`Compiler::extends_installed_at`), so a `def` or a
/// `remove_method` written above the `extend` cannot see the module's hooks
/// and a class method it supplies is not callable yet.
///
/// What is left here is an edge with no statement to hang off: a BUILTIN's
/// own `zeo_abi::BUILTIN_EXTENDS` row, which is true before line 1.
fn boot_extends(compiler: &crate::compiler::Compiler, class: crate::compiler::ClassId) -> Vec<u32> {
    compiler.classes[class.0 as usize]
        .extends
        .iter()
        .filter(|m| !compiler.extend_sites.contains_key(&(class, **m)))
        .map(|m| m.0)
        .collect()
}

pub(crate) fn collect_classes(em: &mut Emitter, analyzed: &Analyzed) -> CResult<CollectedClasses> {
    let compiler = &analyzed.compiler;
    let mut classes = Vec::new();
    let mut methods = Vec::new();
    let mut module_methods = Vec::new();
    let mut class_methods = Vec::new();
    let mut own_cm = Vec::new();
    let mut foreign = Vec::new();
    let mut own_rows = Vec::new();
    let mut vis = Vec::new();
    let mut extends: Vec<(u32, Vec<u32>)> = Vec::new();
    let mut sst: Vec<(u32, u32, String, cranelift_module::FuncId)> = Vec::new();
    let mut alias_rows: Vec<(u32, String, String, bool, u32, bool)> = Vec::new();
    let mut conceal: Vec<u32> = Vec::new();
    let mut singleton_surrogates: Vec<(u32, u32)> = Vec::new();
    let mut redefs: Vec<RedefSpec> = Vec::new();
    let mut set_ancestors: Vec<(u32, Vec<u32>)> = Vec::new();
    let mut register_builtin: Vec<(u32, String, bool, Vec<u32>)> = Vec::new();
    // Object, Kernel, BasicObject -- the tail every ancestry ends with, so a
    // definition here is inherited by every class in the program. `ancestors`
    // starts with the class itself, so this is exactly those three.
    let universal_spine: Vec<ClassId> = compiler.class(crate::compiler::OBJECT_CLASS).ancestors.clone();
    // A definition's one emitted trampoline, keyed by the SCOPE that
    // defined it -- never by name, which two owners can share. Filled as
    // each owner's own row is declared, and read by the carriers that
    // inherit it. An owner declared AFTER its carrier simply misses and
    // the carrier keeps its own copy, the same way the spine lookup
    // behaves.
    let mut shared_bodies: std::collections::HashMap<u32, cranelift_module::FuncId> =
        std::collections::HashMap::new();
    // Builtin-source alias rows, every class including the toplevel (the
    // boxed-overlay target case is refused
    // with its class). A require-gated builtin whose feature never fired
    // registers nothing, aliases included.
    for (idx, class) in compiler.classes.iter().enumerate() {
        if class.builtin_aliases.is_empty() && class.class_aliases.is_empty() {
            continue;
        }
        if (class.is_builtin || class.is_bootstrap)
            && !compiler.builtin_is_reachable(zeo_abi::ClassId(idx as u32))
        {
            continue;
        }
        // A per-box builtin OVERLAY registers no entry of its own, so an
        // alias naming it named a class the runtime has never heard of --
        // and the registrar's `expect` on that lookup ABORTED the process.
        // The method rows already redirect to the root the overlay patches
        // (`target`, below); these have to as well.
        let owner = class
            .builtin_overlay
            .map_or(idx as u32, |root| root.0);
        for (new, old, eager) in &class.builtin_aliases {
            alias_rows.push((owner, new.clone(), old.clone(), false, class.box_id, *eager));
        }
        for (new, old) in &class.class_aliases {
            alias_rows.push((owner, new.clone(), old.clone(), true, class.box_id, false));
        }
    }

    // Builtin registry patches, two emission points:
    //
    // - a BOOTSTRAP (exception) class whose own mixin changed its chain
    //   patches it; one whose computed chain merely differs keeps what
    //   `with_core` gave it;
    // - an always-on builtin with its DEFAULT ancestors is registered once
    //   by `register_builtins`, so nothing is emitted for it, while a
    //   require-GATED extension registers per program and a reopen that
    //   CHANGED the ancestry PATCHES the existing entry -- a full
    //   re-register carries no constructor and would take `.new` away.
    for (idx, class) in compiler.classes.iter().enumerate() {
        let cid = crate::compiler::ClassId(idx as u32);
        let chain = || class.ancestors.iter().map(|a| a.0).collect::<Vec<u32>>();
        if class.is_bootstrap && !(class.mixin_order.is_empty()) {
            set_ancestors.push((idx as u32, chain()));
        }
        if !((class.is_builtin || idx == 0)
            && compiler.builtin_is_reachable(zeo_abi::ClassId(cid.0)))
            || class.builtin_overlay.is_some()
            || class.box_id != 0
        {
            continue;
        }
        if zeo_abi::is_gated_builtin(cid) {
            register_builtin.push((idx as u32, compiler.fq_name(cid), class.is_module, chain()));
            // ...and it starts CONCEALED. A require-gated extension's constant
            // does not exist until its `require` runs, which is a POSITION in
            // the program, not a whole-program fact -- `HirNode::FeatureLoaded`
            // reveals it there. The compile-time gate above is the other
            // question: a feature nothing requires anywhere registers nothing
            // at all.
            if !crate::lower::features::is_preloaded_at_boot(
                zeo_abi::builtin_class(cid)
                    .and_then(|b| b.feature)
                    .unwrap_or(""),
            ) {
                conceal.push(idx as u32);
            }
        } else if class.ancestors != zeo_abi::declared_ancestors(cid) {
            set_ancestors.push((idx as u32, chain()));
        }
    }

    // REOPENED builtins first: a
    // non-bootstrap builtin's user methods ride the VALUE channel on the
    // builtin's own id (they dispatch FIRST, before the native table); a
    // BOOTSTRAP (exception) reopen's ride the OBJECT channel as deltas
    // over the native set `with_core` installed. Bodies take a
    // `RubyValue` self, so ivars are name-keyed (`dyn_ivars`).
    //
    // A USER module's own bodies are declared here, before any builtin can
    // carry one. The two carrier walks want opposite orders -- a user class
    // reuses `Kernel`'s body (declared below) and a builtin reuses an
    // included user module's (declared in the walk after it) -- so no
    // single order satisfies both. Declaring is what breaks the cycle:
    // `declare_function` is keyed by SYMBOL and idempotent, so the module's
    // own walk asks for the same name and gets the same id back.
    //
    // Every skip here is the conservative direction. Skipping something the
    // module later declares only costs the sharing; declaring something it
    // never defines would not link, and cannot happen -- the module walk
    // reaches its declaration under exactly the two scope tests repeated
    // below, and its remaining exits are refusals that end the compile.
    for (idx, class) in compiler.classes.iter().enumerate() {
        if idx == 0 || class.is_builtin || class.is_bootstrap || !class.is_module {
            continue;
        }
        if class.feature_gate.is_some() {
            continue;
        }
        let sym = super::names::boxed_owner(
            &compiler.fq_name(crate::compiler::ClassId(idx as u32)),
            class.box_id,
        );
        for &sid in &class.own_methods {
            let scope = compiler.scope(sid);
            if scope.native_default || scope.runtime_conditional {
                continue;
            }
            if super::emit::check_params(&scope.params).is_err() {
                continue;
            }
            let tramp = em
                .module
                .declare_function(
                    &em.pkg_symbol(names::trampoline_symbol(&sym, &scope.name)),
                    Linkage::Local,
                    &params::value_fn_sig(em),
                )
                .map_err(|e| {
                    CodegenError::internal(format!("declaring {sym}#{}: {e}", scope.name))
                })?;
            shared_bodies.insert(sid.0, tramp);
        }
    }

    // ANCESTORS FIRST, not ClassId order. `shared_bodies` is filled by the
    // owner and read by its carriers, so an owner walked after them shares
    // nothing -- and ClassId says nothing about ancestry: `Kernel` is 25
    // while `Integer` is 1. Ancestor COUNT is a topological order of the
    // ancestry itself: an ancestor's own chain is a proper subset of its
    // descendant's, so it is always strictly shorter. Ties keep ClassId
    // order, which is what the rows below have always been written in.
    let mut ancestors_first: Vec<usize> = (0..compiler.classes.len()).collect();
    ancestors_first.sort_by_key(|&i| (compiler.classes[i].ancestors.len(), i));
    for idx in ancestors_first {
        let class = &compiler.classes[idx];
        // `Object` joins for its CLASS methods only: `class Object; def
        // self.method_added; end` is an ordinary class-method row, while its
        // instance methods are the top-level `def`s `collect_methods`
        // already emitted.
        if !(class.is_builtin || class.is_bootstrap || idx == 0) {
            continue;
        }
        // The entries this id will actually carry. A BOOTSTRAP (exception)
        // reopen keeps only its DELTAS -- a body defined on a native-backed
        // class, or on a module mixed in ABOVE
        // Object; a top-level `include M` reaches
        // every exception through Object and is deliberately nobody's
        // delta. Guards below fire only when something will emit.
        let deltas: Vec<&crate::compiler::MethodEntry> = class
            .methods
            .iter()
            .filter(|e| {
                if idx == 0 {
                    return false;
                }
                let scope = compiler.scope(e.def);
                if scope.native_default {
                    return false;
                }
                if !class.is_bootstrap {
                    return true;
                }
                let dc = e.defined_class(compiler);
                let above_object = {
                    let anc = &class.ancestors;
                    let cut = anc
                        .iter()
                        .position(|&a| a == crate::compiler::OBJECT_CLASS)
                        .unwrap_or(anc.len());
                    &anc[..cut]
                };
                compiler.is_native_backed(dc)
                    || (compiler.class(dc).is_module && above_object.contains(&dc))
            })
            .collect();
        let cms: Vec<&crate::compiler::MethodEntry> = class
            .class_methods
            .iter()
            .filter(|e| !compiler.scope(e.def).native_default)
            .collect();
        if deltas.is_empty() && cms.is_empty() {
            continue;
        }
        let name = compiler.fq_name(crate::compiler::ClassId(idx as u32));
        // A per-BOX class shares its ruby name with the main-box one, so
        // the symbols it declares carry the box; the frame label keeps the
        // ruby name.
        let sym = super::names::boxed_owner(&name, class.box_id);
        // A require-gated builtin whose feature never fired: no code can
        // resolve its constant, so its rows would be dead weight -- it is
        // skipped entirely (the register-only-enabled-features rule).
        if !compiler.builtin_is_reachable(zeo_abi::ClassId(idx as u32)) {
            continue;
        }
        // A per-box OVERLAY never registers an entry of its own -- instances
        // keep the ROOT builtin's identity -- but its rows still register,
        // on the root's entry keyed by the box. A core constant ALIAS
        // (`Errno::EWOULDBLOCK` IS `Errno::EAGAIN`) is the same shape with
        // no box: its rows are the root's, which is what makes rescuing by
        // either name catch the other.
        let target = class
            .builtin_overlay
            .map_or(crate::compiler::ClassId(idx as u32), |root| root);
        // No separate own-row marking here: a VALUE row self-records
        // ownership at insert (`own_value_names`), and a bootstrap delta's
        // object-channel row marks nothing for a reopen either.
        for entry in deltas {
            let scope = compiler.scope(entry.def);
            let mname = compiler.names.str(entry.name).to_string();
            let refuse_m = |what: &str| {
                Err(CodegenError::unsupported(
                    format!("the CLIF backend cannot lower {what} yet ({name}#{mname})"),
                    scope.def_node.and_then(|n| compiler.hir.span(n)),
                ))
            };
            let dc = entry.defined_class(compiler);
            // Through the same redirect `target` took: a per-box OVERLAY's own
            // `def` has the overlay as its defining class and registers on the
            // root, which read as "a module's method materialized here" and
            // marked the row FOREIGN -- so the box's own method was not its
            // own, and `instance_methods(false)`, `#owner` and `remove_method`
            // all said it did not exist.
            let dc_root = compiler.class(dc).builtin_overlay.unwrap_or(dc);
            if !class.is_bootstrap && dc_root != target {
                // A module method materialized onto this builtin: the row
                // registers here (compiled in this class's context) and is
                // marked FOREIGN so `super` skips this position.
                foreign.push((target.0, mname.clone()));
            }
            // A conditional `def` emits a RUNTIME install at its document
            // position instead (analyze left it in the body's statements),
            // so it contributes no static row here.
            if scope.runtime_conditional {
                continue;
            }

            let p = &scope.params;
            if let Err(what) = super::emit::check_params(p) {
                return refuse_m(what);
            }
            let layout = super::params::layout_of(p)?;
            let has_blk = scope.needs_block_param();
            // A FOREIGN row -- a module method materialized onto this
            // builtin -- names the module's one body when it touches no
            // ivar. Both are name-keyed (`dyn_ivars: true` below and on the
            // module's own row), so there is nothing per-carrier left.
            //
            // This is where `Kernel#URI` was costing 85 copies: the spine
            // lookup only covers TOP-LEVEL defs, and every builtin carrier
            // of a `def` written inside `module Kernel` took its own. It
            // must sit BEFORE the declarations below -- a declared-but-
            // undefined local function does not link.
            if dc != target
                && class.box_id == 0
                && !scope_names_an_ivar(compiler, entry.def)
                && let Some(shared) = shared_bodies.get(&entry.def.0).copied()
            {
                match scope.visibility {
                    crate::hir::Visibility::Private => vis.push(statics::VisRowSpec {
                        class: target.0,
                        name: mname.clone(),
                        verb: 0,
                    }),
                    crate::hir::Visibility::Protected => vis.push(statics::VisRowSpec {
                        class: target.0,
                        name: mname.clone(),
                        verb: 1,
                    }),
                    crate::hir::Visibility::Public => {}
                }
                // A BOOTSTRAP carrier (an exception class) takes the object
                // channel, everything else the value channel -- the same
                // split the ordinary rows below make.
                if class.is_bootstrap {
                    methods.push(ObjMethodSpec {
                        is_own: false,
                        super_target_only: false,
                        dyn_ivars: true,
                        alias_of: scope.alias_of.clone(),
                        defining_class: scope.defining_class,
                        lexical_home: scope.lexical_home,
                        owner: target,
                        owner_name: name.clone(),
                        name: mname,
                        body: scope.body.clone(),
                        node: scope.def_node,
                        tramp: shared,
                        accessor: None,
                        body_fn: None,
                        hir_params: p.clone(),
                        has_blk,
                        ruby2_keywords: scope.ruby2_keywords,
                        shared: true,
                    });
                } else {
                    module_methods.push(ModMethodSpec {
                        dyn_ivars: true,
                        box_id: class.box_id,
                        alias_of: scope.alias_of.clone(),
                        defining_class: scope.defining_class,
                        lexical_home: scope.lexical_home,
                        owner: target,
                        owner_name: name.clone(),
                        name: mname,
                        body: scope.body.clone(),
                        node: scope.def_node,
                        tramp: shared,
                        body_fn: shared,
                        hir_params: p.clone(),
                        has_blk,
                        ruby2_keywords: scope.ruby2_keywords,
                        shared: true,
                    });
                }
                continue;
            }
            let tramp = em
                .module
                .declare_function(
                    &em.pkg_symbol(names::trampoline_symbol(&sym, &mname)),
                    Linkage::Local,
                    &params::value_fn_sig(em),
                )
                .map_err(|e| CodegenError::internal(format!("declaring {name}#{mname}: {e}")))?;
            let sig = params::body_sig(em, layout.n_slots, has_blk);
            let body_fn = em
                .module
                .declare_function(&em.pkg_symbol(names::method_symbol(&sym, &mname)), Linkage::Local, &sig)
                .map_err(|e| CodegenError::internal(format!("declaring {name}#{mname}: {e}")))?;
            match scope.visibility {
                crate::hir::Visibility::Private => vis.push(statics::VisRowSpec {
                    class: target.0,
                    name: mname.clone(),
                    verb: 0,
                }),
                crate::hir::Visibility::Protected => vis.push(statics::VisRowSpec {
                    class: target.0,
                    name: mname.clone(),
                    verb: 1,
                }),
                crate::hir::Visibility::Public => {}
            }
            // The builtin's OWN write is what a carrier reuses. `Kernel#URI`
            // -- a gem reopening `Kernel` -- reached 85 classes and every
            // one took a copy: the spine lookup asks `em.methods`, which
            // `collect::collect_methods` fills from TOP-LEVEL defs only, so
            // a `def` written inside `module Kernel` was never in it.
            if dc == target {
                shared_bodies.insert(entry.def.0, tramp);
            }
            if class.is_bootstrap {
                // An exception reopen: an OBJECT-channel delta.
                methods.push(ObjMethodSpec {
                    is_own: class.own_methods.contains(&entry.def),
                    super_target_only: false,
                    dyn_ivars: true,
                    alias_of: scope.alias_of.clone(),
                    defining_class: scope.defining_class,
                    lexical_home: scope.lexical_home,
                    owner: target,
                    owner_name: name.clone(),
                    name: mname,
                    body: scope.body.clone(),
                    node: scope.def_node,
                    tramp,
                    accessor: None,
                    body_fn: Some(body_fn),
                    hir_params: p.clone(),
                    has_blk,
                    ruby2_keywords: scope.ruby2_keywords,
                    shared: false,
                });
            } else {
                // A value-channel row on the builtin's own id.
                module_methods.push(ModMethodSpec {
                    dyn_ivars: true,
                    box_id: class.box_id,
                    alias_of: scope.alias_of.clone(),
                    defining_class: scope.defining_class,
                    lexical_home: scope.lexical_home,
                    owner: target,
                    owner_name: name.clone(),
                    name: mname,
                    body: scope.body.clone(),
                    node: scope.def_node,
                    tramp,
                    body_fn,
                    hir_params: p.clone(),
                    has_blk,
                    ruby2_keywords: scope.ruby2_keywords,
                shared: false,
                });
            }
        }
        let mut cm_def_tramps: Vec<(crate::compiler::ScopeId, cranelift_module::FuncId)> =
            Vec::new();
        for entry in cms {
            let scope = compiler.scope(entry.def);
            let mname = compiler.names.str(entry.name).to_string();
            let refuse_m = |what: &str| {
                Err(CodegenError::unsupported(
                    format!("the CLIF backend cannot lower {what} yet ({name}.{mname})"),
                    scope.def_node.and_then(|n| compiler.hir.span(n)),
                ))
            };
            if scope.runtime_conditional {
                continue;
            }

            let p = &scope.params;
            if let Err(what) = super::emit::check_params(p) {
                return refuse_m(what);
            }
            let layout = super::params::layout_of(p)?;
            let has_blk = scope.needs_block_param();
            let tramp = em
                .module
                .declare_function(
                    &em.pkg_symbol(names::class_trampoline_symbol(&sym, &mname)),
                    Linkage::Local,
                    &params::value_fn_sig(em),
                )
                .map_err(|e| CodegenError::internal(format!("declaring {name}.{mname}: {e}")))?;
            let sig = params::body_sig(em, layout.n_slots, has_blk);
            let body_fn = em
                .module
                .declare_function(
                    &em.pkg_symbol(names::class_method_symbol(&sym, &mname)),
                    Linkage::Local,
                    &sig,
                )
                .map_err(|e| CodegenError::internal(format!("declaring {name}.{mname}: {e}")))?;
            if entry.visibility == crate::hir::Visibility::Private {
                vis.push(statics::VisRowSpec {
                    class: target.0,
                    name: mname.clone(),
                    verb: 3,
                });
            }
            class_methods.push(CmMethodSpec {
                cm_row: true,
                box_id: class.box_id,
                alias_of: scope.alias_of.clone(),
                // A `def` written in a `class << self` body was written in
                // the SINGLETON, so that is its cref -- which is where a
                // `def` nested inside it installs, and what its bare
                // constants and `Module.nesting` resolve through.
                defining_class: scope.lexical_home.unwrap_or(scope.defining_class),
                lexical_home: scope.lexical_home,
                owner: target,
                owner_name: name.clone(),
                name: mname,
                body: scope.body.clone(),
                node: scope.def_node,
                tramp,
                body_fn,
                hir_params: p.clone(),
                has_blk,
                ruby2_keywords: scope.ruby2_keywords,
            });
            cm_def_tramps.push((entry.def, tramp));
        }
        emit_singleton_super_targets(
            compiler,
            em,
            class,
            target,
            &name,
            &sym,
            &cm_def_tramps,
            &mut sst,
            &mut class_methods,
        )?;
        for &sid in &class.own_class_methods {
            if compiler.scope(sid).native_default {
                continue;
            }
            own_cm.push((target.0, compiler.scope(sid).name.clone()));
        }
    }

    for (idx, class) in compiler.classes.iter().enumerate() {
        if idx == 0 || class.is_builtin || class.is_bootstrap {
            // A builtin needs no registrar of its own, but its singleton
            // chain still does: `CGI` EXTENDS the module it includes
            // (`zeo_abi::BUILTIN_EXTENDS`), and without the row
            // `CGI.escapeHTML` reaches no body at all.
            let boot = boot_extends(compiler, crate::compiler::ClassId(idx as u32));
            if !boot.is_empty() {
                extends.push((idx as u32, boot));
            }
            continue;
        }
        // EXPERIMENTAL (M2): an interface-registered class emits NOTHING --
        // its desc rows and bodies travel in the package object and merge
        // in `pkg::merge_rows`. What the host does own is the typed
        // direct-call table: each plain exported body is declared as an
        // IMPORT here, so a host call site on a packaged receiver goes
        // direct into the package's compiled code.
        if class.imported_pkg.is_some() {
            // Every (package, local id) pair aliased onto this class: a
            // shared namespace maps two packages' locals to one host id,
            // and each method's tramp lives in whichever package's
            // manifest defined it.
            let pairs: Vec<(u32, u32)> = compiler
                .pkg_class_map
                .iter()
                .filter(|(_, v)| v.0 == idx as u32)
                .map(|((mp, l), _)| (*mp, *l))
                .collect();
            for sid in &class.own_methods {
                let scope = compiler.scope(*sid);
                let Some(sym) = scope.extern_symbol.as_deref() else {
                    continue;
                };
                if sym.is_empty() {
                    continue;
                }
                let layout = params::layout_of(&scope.params)?;
                if !layout.plain || class.box_id != 0 {
                    continue;
                }
                let tramp_sym = pairs.iter().find_map(|(mp, l)| {
                    compiler.hir.pkg_merge[*mp as usize]
                        .obj
                        .iter()
                        .find(|r| r.class == *l && r.name == scope.name)
                        .map(|r| r.f.clone())
                });
                let Some(tramp_sym) = tramp_sym else { continue };
                let has_blk = scope.needs_block_param();
                let body = em
                    .module
                    .declare_function(
                        sym,
                        Linkage::Import,
                        &params::body_sig(em, layout.n_slots, has_blk),
                    )
                    .map_err(|e| {
                        CodegenError::internal(format!("importing {sym}: {e}"))
                    })?;
                let tramp = em
                    .module
                    .declare_function(&tramp_sym, Linkage::Import, &params::value_fn_sig(em))
                    .map_err(|e| {
                        CodegenError::internal(format!("importing {tramp_sym}: {e}"))
                    })?;
                if crate::debug_flags::debug(crate::debug_flags::DebugFlag::TraceTyped) {
                    eprintln!("extern typed method ({}, {})", idx, scope.name);
                }
                em.typed_methods.insert(
                    (idx as u32, scope.name.clone()),
                    super::module::MethodDecl {
                        body,
                        tramp,
                        arity: scope.params.required.len(),
                        plain: layout.plain,
                        kw_direct: layout.kw_direct.clone(),
                        has_blk,
                        reopen_flagged: false,
                        concealed: false,
                    },
                );
            }
            continue;
        }
        let name = compiler.fq_name(crate::compiler::ClassId(idx as u32));
        // A per-BOX class shares its ruby name with the main-box one, so
        // the symbols it declares carry the box; the frame label keeps the
        // ruby name.
        let sym = super::names::boxed_owner(&name, class.box_id);
        let refuse = |what: &str| {
            Err(CodegenError::unsupported(
                format!("the CLIF backend cannot lower {what} yet (class {name})"),
                compiler.class_def_span(crate::compiler::ClassId(idx as u32)),
            ))
        };

        // A runtime-CONDITIONAL class registers its shape (the static MRO
        // needs one) but starts CONCEALED: the constant does not exist
        // until the guarded body runs and reveals it.
        if class.runtime_conditional
            || compiler.class_waits_for_its_unit(crate::compiler::ClassId(idx as u32))
        {
            conceal.push(idx as u32);
        }
        // A `class << self` body is homed on a surrogate module; seeding
        // the runtime mint is what makes `Owner.singleton_class` answer it.
        if compiler.is_singleton_surrogate(crate::compiler::ClassId(idx as u32))
            && let Some(owner) = class.lexical_parent
        {
            singleton_surrogates.push((idx as u32, owner.0));
        }
        // `include` works through the two mechanisms below (materialized
        // copies on the includer + value rows on the module + the module in
        // `ancestors`), and `singleton_class.prepend` through the same
        // materialization the instance side uses -- analyze has already put
        // the winners in `class_methods` and every position's copy in
        // `singleton_super_targets`. A refinement's `import_methods` needs
        // nothing here either: the RUNTIME copies the rows when the holder
        // body's call runs, and `imported_modules` exists only so a refined
        // call site is nominated for a name the import brought in.
        // The rest of the class surface needs no refusal: `pending_aliases`,
        // `pending_module_functions` and `class_undefined` are DRAINED or
        // applied by analyze, `runtime_undefs` is compile-side fold
        // suppression, and `undefined`/visibility overrides emit as rows in
        // the uniform pass below.
        if class.feature_gate.is_some() {
            return refuse("a feature-gated class");
        }
        // The registrar this class needs. An exception subclass's chain
        // holds builtin superclasses BY DESIGN -- the registrar installs
        // the native default method set and the class's own defs layer
        // over it as deltas.
        let cid = ClassId(idx as u32);
        let native_backed = compiler.is_native_backed(cid);
        let immediate = compiler.is_immediate_subclass(cid);
        // The registrar chain, in its order.
        let kind = if class.is_module {
            zeo_abi::abi::CLASS_MODULE
        } else if compiler.is_exception_backed(cid) {
            zeo_abi::abi::CLASS_EXCEPTION
        } else if compiler.is_value_subclass(cid) {
            zeo_abi::abi::CLASS_VALUE_SUBCLASS
        } else if compiler.is_date_subclass(cid) || compiler.is_proc_subclass(cid) {
            zeo_abi::abi::CLASS_RECV_HONOURING
        } else if compiler.is_weakmap_subclass(cid) {
            zeo_abi::abi::CLASS_WEAK_MAP
        } else if compiler.is_module_subclass(cid) {
            zeo_abi::abi::CLASS_MODULE_SUBCLASS
        } else if immediate {
            zeo_abi::abi::CLASS_IMMEDIATE
        } else {
            zeo_abi::abi::CLASS_PLAIN
        };
        // Prepend winners flow through the MATERIALIZED `methods` table
        // (analyze picked them; `ancestors` already orders the module
        // before the class), so a prepend that shadows NOTHING needs no
        // emission of its own whatever the registrar. Only a SHADOWED own
        // def does, and on a native-backed class its body cannot register
        // as an object-channel bridge -- that shape needs an own-impl
        // promotion pass, not built yet.
        if class.has_prepends() && kind != zeo_abi::abi::CLASS_PLAIN {
            let winners: std::collections::HashSet<crate::compiler::ScopeId> =
                class.methods.iter().map(|e| e.def).collect();
            let shadows_own = class.own_methods.iter().any(|sid| {
                let scope = compiler.scope(*sid);
                !winners.contains(sid) && !scope.native_default && !scope.runtime_conditional
            });
            if shadows_own {
                return refuse("a prepend that shadows an own def on a native-backed class");
            }
        }
        // Only the edges NO statement installs. A class body`s `extend M`,
        // and the `base.extend(ClassMethods)` an `included` hook performs, are
        // seated at their own statement instead (`extends_installed_at`) --
        // ruby puts the module in the singleton chain where the statement
        // stands, not from program start.
        let boot: Vec<u32> = boot_extends(compiler, crate::compiler::ClassId(idx as u32));
        if !boot.is_empty() {
            extends.push((idx as u32, boot));
        }
        // Every ancestor past self must be a user class or the plain
        // Object/Kernel/BasicObject spine -- a builtin superclass outside
        // the shapes above selects a registrar the slice does not emit.
        for &a in class
            .ancestors
            .iter()
            .skip(1)
            .filter(|_| kind == zeo_abi::abi::CLASS_PLAIN)
        {
            let plain_spine = matches!(a.0, 0 | 24 | 25);
            // The two ordinary-object builtins from the payload-root
            // DENYLIST that no other registrar claims (`Numeric`,
            // `WeakRef`): a subclass registers as a plain class, and
            // inherited behavior comes from the walk probing
            // their builtin tables at MRO position (`Date::Infinity <
            // Numeric` is the corpus shape).
            // `Struct`/`Data` are on the payload-root denylist for the same
            // reason: their subclasses are ORDINARY ivar objects whose
            // members are hidden ivars, so the plain registrar serves them
            // and `register_compiled_struct` (below) hands `Struct`'s one
            // shared protocol the member list.
            let plain_builtin = matches!(
                a,
                zeo_abi::NUMERIC_CLASS
                    | zeo_abi::WEAKREF_CLASS
                    | zeo_abi::STRUCT_CLASS
                    | zeo_abi::DATA_CLASS
                    | zeo_abi::FFI_STRUCT_CLASS
                    | zeo_abi::FFI_UNION_CLASS
            );
            let user = (a.0 as usize) < compiler.classes.len()
                && !compiler.classes[a.0 as usize].is_builtin
                && !compiler.classes[a.0 as usize].is_bootstrap;
            // A builtin MODULE in the chain (`include Comparable`) is fine:
            // registration carries the id and the dispatch walk probes the
            // module's builtin table at its MRO position, exactly as it does
            // for Kernel. Only a builtin SUPERCLASS selects a registrar the
            // slice does not emit yet.
            let builtin_module =
                (a.0 as usize) < compiler.classes.len() && compiler.classes[a.0 as usize].is_module;
            if !(plain_spine || plain_builtin || user || builtin_module) {
                return refuse("a builtin superclass");
            }
        }

        classes.push(ClassSpec {
            id: idx as u32,
            name: name.clone(),
            ancestors: class.ancestors.iter().map(|c| c.0).collect(),
            ivars: class.ivars.clone(),
            hidden: u16::try_from(class.hidden_ivars.len()).expect("hidden ivars fit u16"),
            members: class.hidden_ivars.clone(),
            kind,
        });
        // What this class's own body wrote -- `instance_methods(false)` /
        // `Method#owner` truth.
        // A conditional `def` is left out: `instance_methods(false)` must
        // report what the class HAS, and whether it has this one is settled
        // at run time by the row its install writes.
        //
        // A method this class only RE-SCOPED (`private :inherited_method`)
        // is its own too: ruby plants a real entry for it (CRuby's
        // `VM_METHOD_TYPE_ZSUPER`), which is what makes the name answer
        // `private_instance_methods(false)` and `instance_method(:x).owner`
        // here while still running the ancestor's body.
        let mut own: Vec<String> = class
            .own_methods
            .iter()
            .filter(|&&sid| !compiler.scope(sid).runtime_conditional)
            .map(|&sid| compiler.scope(sid).name.clone())
            .chain(
                compiler
                    .methods_of(ClassId(idx as u32))
                    .iter()
                    .filter(|e| e.zsuper)
                    .map(|e| compiler.names.str(e.name).to_string()),
            )
            .collect();
        own.sort();
        for n in own {
            own_rows.push((idx as u32, n));
        }

        if class.is_module {
            // A module's own methods ride the VALUE channel on its own id.
            for &sid in &class.own_methods {
                let scope = compiler.scope(sid);
                if scope.native_default {
                    continue;
                }
                let mname = scope.name.clone();
                let refuse_m = |what: &str| {
                    Err(CodegenError::unsupported(
                        format!("the CLIF backend cannot lower {what} yet ({name}#{mname})"),
                        scope.def_node.and_then(|n| compiler.hir.span(n)),
                    ))
                };
                if scope.runtime_conditional {
                    continue;
                }

                let p = &scope.params;
                if let Err(what) = super::emit::check_params(p) {
                    return refuse_m(what);
                }
                let layout = super::params::layout_of(p)?;
                let has_blk = scope.needs_block_param();
                let tramp = em
                    .module
                    .declare_function(
                        &em.pkg_symbol(names::trampoline_symbol(&sym, &mname)),
                        Linkage::Local,
                        &params::value_fn_sig(em),
                    )
                    .map_err(|e| {
                        CodegenError::internal(format!("declaring {name}#{mname}: {e}"))
                    })?;
                let sig = params::body_sig(em, layout.n_slots, has_blk);
                let body_fn = em
                    .module
                    .declare_function(&em.pkg_symbol(names::method_symbol(&sym, &mname)), Linkage::Local, &sig)
                    .map_err(|e| {
                        CodegenError::internal(format!("declaring {name}#{mname}: {e}"))
                    })?;
                match scope.visibility {
                    crate::hir::Visibility::Private => vis.push(statics::VisRowSpec {
                        class: idx as u32,
                        name: mname.clone(),
                        verb: 0,
                    }),
                    crate::hir::Visibility::Protected => vis.push(statics::VisRowSpec {
                        class: idx as u32,
                        name: mname.clone(),
                        verb: 1,
                    }),
                    crate::hir::Visibility::Public => {}
                }
                // This is the body an includer reuses when its own copy
                // would be identical -- see the sharing note below.
                shared_bodies.insert(sid.0, tramp);
                module_methods.push(ModMethodSpec {
                    box_id: class.box_id,
                    // A module's VALUE-channel row takes whatever receiver
                    // dispatch hands over, so its ivars are name-keyed (the
                    // includer's materialized object-channel copy keeps the
                    // slot-indexed fast path).
                    dyn_ivars: true,
                    alias_of: scope.alias_of.clone(),
                    defining_class: scope.defining_class,
                    lexical_home: scope.lexical_home,
                    owner: ClassId(idx as u32),
                    owner_name: name.clone(),
                    name: mname,
                    body: scope.body.clone(),
                    node: scope.def_node,
                    tramp,
                    body_fn,
                    hir_params: p.clone(),
                    has_blk,
                    ruby2_keywords: scope.ruby2_keywords,
                shared: false,
                });
            }
        }

        // An immediate-builtin subclass has NO instances -- no method rows
        // are emitted for it (the definition is allowed, `.new` raises).
        let module_subclass = kind == zeo_abi::abi::CLASS_MODULE_SUBCLASS;
        for entry in class
            .methods
            .iter()
            .filter(|_| !class.is_module && !immediate)
        {
            let scope = compiler.scope(entry.def);
            if scope.native_default {
                continue;
            }
            let mname = compiler.names.str(entry.name).to_string();
            let refuse_m = |what: &str| {
                Err(CodegenError::unsupported(
                    format!("the CLIF backend cannot lower {what} yet ({name}#{mname})"),
                    scope.def_node.and_then(|n| compiler.hir.span(n)),
                ))
            };
            // A conditional `def` emits a RUNTIME install at its document
            // position instead (analyze left it in the body's statements),
            // so it contributes no static row here.
            if scope.runtime_conditional {
                continue;
            }

            let p = &scope.params;
            if let Err(what) = super::emit::check_params(p) {
                return refuse_m(what);
            }
            let layout = super::params::layout_of(p)?;
            // A native-backed instance has NO slots: its accessor runs as
            // an ordinary body (a lone name-keyed ivar read/write).
            let accessor = match (
                compiler.accessor_shape(ClassId(idx as u32), scope),
                native_backed,
            ) {
                (Some(_), true) | (None, _) => None,
                (Some(shape), false) => {
                    let slot = crate::analyze::class_query::slot_of(
                        compiler,
                        ClassId(idx as u32),
                        &shape.ivar,
                    )
                    .ok_or_else(|| {
                        CodegenError::internal(format!(
                            "accessor ivar @{} has no slot on {name}",
                            shape.ivar
                        ))
                    })?;
                    Some((slot, shape.kind, shape.attr_generated))
                }
            };
            let has_blk = scope.needs_block_param();
            // A definition on the UNIVERSAL spine is already emitted once by
            // `collect::collect_methods`, under its `Object#name` symbol, and
            // every class in the program inherits it. Emitting a copy per
            // class is what a dozen gems reopening `Kernel` or writing a
            // top-level `def` turn into 22,393 bodies on a program that
            // requires rubygems (`--dump=methods`). The row names the one
            // body instead.
            //
            // Safe because these bodies carry nothing per-class, which is
            // measured rather than assumed (see the golden beside this):
            // `clif::ivars::ivar_slot_of` answers NAME-KEYED for an `Object`
            // owner, so there is no slot constant for two carriers to
            // disagree about, and a top-level `def` resolves constants
            // against `Object` however it was reached.
            //
            // Narrow on purpose. An own definition, an accessor and a boxed
            // class each keep their own row.
            // A NATIVE-BACKED carrier is fine here, which is worth stating:
            // its own bodies read name-keyed ivars, and so does an
            // `Object`-owned body, so the two agree. Excluding it left every
            // `Gem::` exception class taking private copies of `Kernel#pp`.
            let shareable = !class.own_methods.contains(&entry.def)
                && accessor.is_none()
                && class.box_id == 0;
            // A definition on an ORDINARY MODULE is the same argument one
            // step out. The module's own value-channel row is already
            // emitted name-keyed -- the includer's object-channel copy
            // exists ONLY to keep the slot-indexed ivar fast path -- so a
            // body that names no ivar has nothing left to disagree about
            // and can name the module's one body.
            //
            // `include M` in three classes emitted FOUR identical bodies
            // before this: one per carrier plus the module's. The copies
            // differed only in per-site inline-cache offsets, and sharing
            // them shares one cache site, which is correct and better. The
            // frame label is the module's under both engines, and constant
            // lookup is lexical from the module, so neither depends on the
            // carrier.
            // A SUPERCLASS is the same argument as a module. Its own body is
            // emitted against ITS OWN slots, so a carrier may name it only
            // where the two agree about every `@x` the body touches --
            // which `ivar_slots_agree` decides per body. What used to differ
            // was the frame label, and that named the wrong class (see
            // `an_inherited_frame_names_the_defining_class`); with it fixed
            // the copies differ only in per-site cache offsets.
            let shared_tramp = if universal_spine.contains(&scope.defining_class) {
                // By NAME for a top-level `def` (what `collect_methods`
                // emits), then by scope for one written inside
                // `module Kernel` -- the second spelling never reaches the
                // first map.
                em.methods
                    .get(&mname)
                    .map(|d| d.tramp)
                    .or_else(|| shared_bodies.get(&entry.def.0).copied())
            } else if ivar_slots_agree(
                compiler,
                scope.defining_class,
                ClassId(idx as u32),
                entry.def,
            ) {
                shared_bodies.get(&entry.def.0).copied()
            } else {
                None
            };
            if shareable
                && let Some(shared) = shared_tramp
            {
                // The visibility row is the class's, not the shared body's,
                // and it is pushed further down the ordinary path -- so it
                // has to be pushed here too. A top-level `def` is PRIVATE in
                // ruby, and skipping this made `respond_to?(:read_const)`
                // answer true where ruby answers false.
                match scope.visibility {
                    crate::hir::Visibility::Private => vis.push(statics::VisRowSpec {
                        class: idx as u32,
                        name: mname.clone(),
                        verb: 0,
                    }),
                    crate::hir::Visibility::Protected => vis.push(statics::VisRowSpec {
                        class: idx as u32,
                        name: mname.clone(),
                        verb: 1,
                    }),
                    crate::hir::Visibility::Public => {}
                }
                methods.push(ObjMethodSpec {
                    is_own: false,
                    super_target_only: false,
                    dyn_ivars: false,
                    alias_of: scope.alias_of.clone(),
                    defining_class: scope.defining_class,
                    lexical_home: scope.lexical_home,
                    owner: ClassId(idx as u32),
                    owner_name: name.clone(),
                    name: mname,
                    body: scope.body.clone(),
                    node: scope.def_node,
                    tramp: shared,
                    accessor: None,
                    body_fn: None,
                    hir_params: p.clone(),
                    has_blk,
                    ruby2_keywords: scope.ruby2_keywords,
                    shared: true,
                });
                continue;
            }
            let tramp = em
                .module
                .declare_function(
                    &em.pkg_symbol(names::trampoline_symbol(&sym, &mname)),
                    Linkage::Local,
                    &params::value_fn_sig(em),
                )
                .map_err(|e| CodegenError::internal(format!("declaring {name}#{mname}: {e}")))?;
            // The class's OWN body is what its subclasses reuse instead of
            // taking a copy. Only an own write registers: a materialized
            // copy is the thing being removed, not a candidate to share.
            if class.own_methods.contains(&entry.def) && accessor.is_none() {
                shared_bodies.insert(entry.def.0, tramp);
            }
            let body_fn = if accessor.is_none() {
                let sig = params::body_sig(em, layout.n_slots, has_blk);
                Some(
                    em.module
                        .declare_function(&em.pkg_symbol(names::method_symbol(&sym, &mname)), Linkage::Local, &sig)
                        .map_err(|e| {
                            CodegenError::internal(format!("declaring {name}#{mname}: {e}"))
                        })?,
                )
            } else {
                None
            };
            let verb = match scope.visibility {
                crate::hir::Visibility::Private => Some(0),
                crate::hir::Visibility::Protected => Some(1),
                crate::hir::Visibility::Public => None,
            };
            if let Some(verb) = verb {
                vis.push(statics::VisRowSpec {
                    class: idx as u32,
                    name: mname.clone(),
                    verb,
                });
            }
            if module_subclass {
                // A `class X < Module` instance is a `RubyValue::Class`: the
                // object channel never sees it, so X's defs register as VALUE
                // rows on X's id; ivars
                // resolve through `ivar_set_dyn`'s Class arm (civars).
                module_methods.push(ModMethodSpec {
                    dyn_ivars: true,
                    box_id: class.box_id,
                    alias_of: scope.alias_of.clone(),
                    defining_class: scope.defining_class,
                    lexical_home: scope.lexical_home,
                    owner: ClassId(idx as u32),
                    owner_name: name.clone(),
                    name: mname,
                    body: scope.body.clone(),
                    node: scope.def_node,
                    tramp,
                    body_fn: body_fn.expect("module-subclass accessors run as bodies"),
                    hir_params: p.clone(),
                    has_blk,
                    ruby2_keywords: scope.ruby2_keywords,
                shared: false,
                });
                continue;
            }
            // The typed direct-call table: a plain compiled body on a
            // main-box class. Accessor rows (no body) fold elsewhere;
            // native-backed instances keep dispatch (their bodies read
            // name-keyed ivars); a boxed class may carry its own patches.
            if let Some(body) = body_fn
                && !native_backed
                && class.box_id == 0
            {
                em.typed_methods.insert(
                    (idx as u32, mname.clone()),
                    super::module::MethodDecl {
                        body,
                        tramp,
                        arity: p.required.len(),
                        plain: layout.plain,
                        kw_direct: layout.kw_direct.clone(),
                        has_blk,
                        reopen_flagged: false,
                        concealed: false,
                    },
                );
            }
            methods.push(ObjMethodSpec {
                is_own: class.own_methods.contains(&entry.def),
                super_target_only: false,
                dyn_ivars: native_backed,
                alias_of: scope.alias_of.clone(),
                defining_class: scope.defining_class,
                lexical_home: scope.lexical_home,
                owner: ClassId(idx as u32),
                owner_name: name.clone(),
                name: mname,
                body: scope.body.clone(),
                node: scope.def_node,
                tramp,
                accessor,
                body_fn,
                hir_params: p.clone(),
                has_blk,
                ruby2_keywords: scope.ruby2_keywords,
                shared: false,
            });
        }
        // An own method a `prepend` SHADOWED never won its name in the
        // materialized table -- its body still compiles, reachable ONLY
        // through the module copy's `super`.
        if class.has_prepends() && !class.is_module && !immediate {
            let winners: std::collections::HashSet<crate::compiler::ScopeId> =
                class.methods.iter().map(|e| e.def).collect();
            for &sid in &class.own_methods {
                if winners.contains(&sid) {
                    continue;
                }
                let scope = compiler.scope(sid);
                if scope.native_default {
                    continue;
                }
                let mname = scope.name.clone();
                let refuse_m = |what: &str| {
                    Err(CodegenError::unsupported(
                        format!("the CLIF backend cannot lower {what} yet ({name}#{mname})"),
                        scope.def_node.and_then(|n| compiler.hir.span(n)),
                    ))
                };
                if scope.runtime_conditional {
                    continue;
                }

                let p = &scope.params;
                if let Err(what) = super::emit::check_params(p) {
                    return refuse_m(what);
                }
                let layout = super::params::layout_of(p)?;
                let accessor = match compiler.accessor_shape(ClassId(idx as u32), scope) {
                    None => None,
                    Some(shape) => {
                        let slot = crate::analyze::class_query::slot_of(
                            compiler,
                            ClassId(idx as u32),
                            &shape.ivar,
                        )
                        .ok_or_else(|| {
                            CodegenError::internal(format!(
                                "accessor ivar @{} has no slot on {name}",
                                shape.ivar
                            ))
                        })?;
                        Some((slot, shape.kind, shape.attr_generated))
                    }
                };
                let has_blk = scope.needs_block_param();
                let tramp = em
                    .module
                    .declare_function(
                        &em.pkg_symbol(names::trampoline_symbol(&sym, &format!("__own_{mname}"))),
                        Linkage::Local,
                        &params::value_fn_sig(em),
                    )
                    .map_err(|e| {
                        CodegenError::internal(format!("declaring {name}#{mname}: {e}"))
                    })?;
                let body_fn = if accessor.is_none() {
                    let sig = params::body_sig(em, layout.n_slots, has_blk);
                    Some(
                        em.module
                            .declare_function(
                                &em.pkg_symbol(names::method_symbol(&sym, &format!("__own_{mname}"))),
                                Linkage::Local,
                                &sig,
                            )
                            .map_err(|e| {
                                CodegenError::internal(format!("declaring {name}#{mname}: {e}"))
                            })?,
                    )
                } else {
                    None
                };
                methods.push(ObjMethodSpec {
                    is_own: true,
                    super_target_only: true,
                    dyn_ivars: false,
                    alias_of: scope.alias_of.clone(),
                    defining_class: scope.defining_class,
                    lexical_home: scope.lexical_home,
                    owner: ClassId(idx as u32),
                    owner_name: name.clone(),
                    name: mname,
                    body: scope.body.clone(),
                    node: scope.def_node,
                    tramp,
                    accessor,
                    body_fn,
                    hir_params: p.clone(),
                    has_blk,
                    ruby2_keywords: scope.ruby2_keywords,
                    shared: false,
                });
            }
        }
        // Class methods (`def self.x`): every entry registers on the
        // CLASS-METHOD channel (a `CmRow`), so both a literal `Foo.run`
        // and a variable-held class dispatch through it. With mixins
        // refused above, `class_methods` is the class's own writes.
        let mut cm_def_tramps: Vec<(crate::compiler::ScopeId, cranelift_module::FuncId)> =
            Vec::new();
        for entry in &class.class_methods {
            let scope = compiler.scope(entry.def);
            if scope.native_default {
                continue;
            }
            let mname = compiler.names.str(entry.name).to_string();
            let refuse_m = |what: &str| {
                Err(CodegenError::unsupported(
                    format!("the CLIF backend cannot lower {what} yet ({name}.{mname})"),
                    scope.def_node.and_then(|n| compiler.hir.span(n)),
                ))
            };
            if scope.runtime_conditional {
                continue;
            }

            let p = &scope.params;
            if let Err(what) = super::emit::check_params(p) {
                return refuse_m(what);
            }
            let layout = super::params::layout_of(p)?;
            let has_blk = scope.needs_block_param();
            let tramp = em
                .module
                .declare_function(
                    &em.pkg_symbol(names::class_trampoline_symbol(&sym, &mname)),
                    Linkage::Local,
                    &params::value_fn_sig(em),
                )
                .map_err(|e| CodegenError::internal(format!("declaring {name}.{mname}: {e}")))?;
            let sig = params::body_sig(em, layout.n_slots, has_blk);
            let body_fn = em
                .module
                .declare_function(
                    &em.pkg_symbol(names::class_method_symbol(&sym, &mname)),
                    Linkage::Local,
                    &sig,
                )
                .map_err(|e| CodegenError::internal(format!("declaring {name}.{mname}: {e}")))?;
            // Only Private gets a row (verb 3).
            if entry.visibility == crate::hir::Visibility::Private {
                vis.push(statics::VisRowSpec {
                    class: idx as u32,
                    name: mname.clone(),
                    verb: 3,
                });
            }
            cm_def_tramps.push((entry.def, tramp));
            class_methods.push(CmMethodSpec {
                cm_row: true,
                box_id: class.box_id,
                alias_of: scope.alias_of.clone(),
                // A `def` written in a `class << self` body was written in
                // the SINGLETON, so that is its cref -- which is where a
                // `def` nested inside it installs, and what its bare
                // constants and `Module.nesting` resolve through.
                defining_class: scope.lexical_home.unwrap_or(scope.defining_class),
                lexical_home: scope.lexical_home,
                owner: ClassId(idx as u32),
                owner_name: name.clone(),
                name: mname,
                body: scope.body.clone(),
                node: scope.def_node,
                tramp,
                body_fn,
                hir_params: p.clone(),
                has_blk,
                ruby2_keywords: scope.ruby2_keywords,
            });
        }
        emit_singleton_super_targets(
            compiler,
            em,
            class,
            ClassId(idx as u32),
            &name,
            &sym,
            &cm_def_tramps,
            &mut sst,
            &mut class_methods,
        )?;
        for &sid in &class.own_class_methods {
            own_cm.push((idx as u32, compiler.scope(sid).name.clone()));
        }
    }
    // Visibility overrides (`private :m` retagging an inherited method) and
    // `undef` marks, every class including the toplevel. Vis rows apply in
    // order, and this pass runs after every per-method stamp above, so the
    // override wins.
    for (idx, class) in compiler.classes.iter().enumerate() {
        if class.redef_scopes.is_empty() {
            continue;
        }
        let name = compiler.fq_name(crate::compiler::ClassId(idx as u32));
        let sym = super::names::boxed_owner(&name, class.box_id);
        collect_redef_scopes(compiler, em, class, idx, &name, &sym, &mut redefs)?;
    }
    let mut undef_rows: Vec<(u32, String)> = Vec::new();
    for (idx, class) in compiler.classes.iter().enumerate() {
        if (class.is_builtin || class.is_bootstrap)
            && !compiler.builtin_is_reachable(zeo_abi::ClassId(idx as u32))
        {
            continue;
        }
        for (mname, v) in &class.visibility_overrides {
            let verb = match v {
                crate::hir::Visibility::Private => 0,
                crate::hir::Visibility::Protected => 1,
                crate::hir::Visibility::Public => 2,
            };
            vis.push(statics::VisRowSpec {
                class: idx as u32,
                name: mname.clone(),
                verb,
            });
        }
        for (mname, v) in &class.class_visibility_overrides {
            let verb = if *v == crate::hir::Visibility::Private {
                3
            } else {
                4
            };
            vis.push(statics::VisRowSpec {
                class: idx as u32,
                name: mname.clone(),
                verb,
            });
        }
        let mut undefined: Vec<&String> = class.undefined.iter().collect();
        undefined.sort();
        for n in undefined {
            undef_rows.push((idx as u32, n.clone()));
        }
    }
    let boot_redefs: Vec<(
        u32,
        String,
        crate::compiler::ScopeId,
        bool,
        crate::hir::Visibility,
    )> = compiler
        .positional_redefs
        .iter()
        .map(|(cid, name, sid, singleton)| {
            (
                cid.0,
                name.clone(),
                *sid,
                *singleton,
                compiler.scope(*sid).visibility,
            )
        })
        .collect();
    Ok(CollectedClasses {
        classes,
        methods,
        module_methods,
        class_methods,
        own_cm,
        foreign,
        own_rows,
        vis,
        extends,
        sst,
        alias_rows,
        undef_rows,
        conceal,
        conceal_methods: conceal_unit_methods(compiler),
        singleton_surrogates,
        redefs,
        boot_redefs,
        set_ancestors,
        register_builtin,
    })
}

/// A class's singleton-chain super
/// targets: every `(module, def)` pair in `singleton_super_targets`,
/// deduped. A pair whose def IS a materialized winner reuses that `CmRow`
/// trampoline (every CLIF class-method body is receiver-generic); a
/// shadowed copy gets its own body under a per-(class,module) symbol.
///
/// Shared by the plain-class loop and the builtin-reopen loop: a REOPENED
/// builtin extended with a module needs exactly the same rows, and
/// `minitest` reopens `Warning` that way.
#[allow(clippy::too_many_arguments)]

/// Declare a trampoline and body function for every body of a method with
/// an observable redefinition timeline -- the superseded ones AND the final
/// one, each installed at its own document position by a spliced
/// `MethodRedefine`.
///
/// A pass of its own rather than a block inside the per-class emission loop,
/// because that loop skips a BUILTIN outright and a builtin reopened twice
/// needs these too. Everything else a builtin reopen needs is already
/// emitted elsewhere; this is the one part it shares with a user class.
fn collect_redef_scopes(
    compiler: &crate::compiler::Compiler,
    em: &mut Emitter,
    class: &crate::compiler::ClassInfo,
    idx: usize,
    name: &str,
    sym: &str,
    redefs: &mut Vec<RedefSpec>,
) -> CResult<()> {
    for &(sid, singleton) in &class.redef_scopes {
        let scope = compiler.scope(sid);
        let mname = scope.name.clone();
        let refuse_r = |what: &str| {
            Err(CodegenError::unsupported(
                format!("the CLIF backend cannot lower {what} yet ({name}#{mname})"),
                scope.def_node.and_then(|n| compiler.hir.span(n)),
            ))
        };
        let p = &scope.params;
        if let Err(what) = super::emit::check_params(p) {
            return refuse_r(what);
        }
        let layout = super::params::layout_of(p)?;
        let has_blk = scope.needs_block_param();
        let suffix = format!("__redef_{}_{mname}", sid.0);
        let tramp = em
            .module
            .declare_function(
                &em.pkg_symbol(names::trampoline_symbol(&sym, &suffix)),
                Linkage::Local,
                &params::value_fn_sig(em),
            )
            .map_err(|e| CodegenError::internal(format!("declaring {name}#{mname}: {e}")))?;
        let body_fn = em
            .module
            .declare_function(
                &em.pkg_symbol(names::method_symbol(&sym, &suffix)),
                Linkage::Local,
                &params::body_sig(em, layout.n_slots, has_blk),
            )
            .map_err(|e| CodegenError::internal(format!("declaring {name}#{mname}: {e}")))?;
        redefs.push(RedefSpec {
            owner: ClassId(idx as u32),
            owner_name: name.to_string(),
            scope: sid,
            name: mname,
            body: scope.body.clone(),
            node: scope.def_node,
            tramp,
            body_fn,
            hir_params: p.clone(),
            has_blk,
            ruby2_keywords: scope.ruby2_keywords,
            singleton,
        });
    }
    Ok(())
}

fn emit_singleton_super_targets(
    compiler: &crate::compiler::Compiler,
    em: &mut Emitter,
    class: &crate::compiler::ClassInfo,
    owner: ClassId,
    name: &str,
    sym: &str,
    cm_def_tramps: &[(crate::compiler::ScopeId, cranelift_module::FuncId)],
    sst: &mut Vec<(u32, u32, String, cranelift_module::FuncId)>,
    class_methods: &mut Vec<CmMethodSpec>,
) -> CResult<()> {
    let mut sst_seen: Vec<(ClassId, crate::compiler::ScopeId)> = Vec::new();
    for &(m, sid) in &class.singleton_super_targets {
        if sst_seen.contains(&(m, sid)) {
            continue;
        }
        sst_seen.push((m, sid));
        let scope = compiler.scope(sid);
        if scope.native_default {
            continue;
        }
        let mname = scope.name.clone();
        if let Some(&(_, tramp)) = cm_def_tramps.iter().find(|(d, _)| *d == sid) {
            sst.push((owner.0, m.0, mname, tramp));
            continue;
        }
        let refuse_m = |what: &str| {
            Err(CodegenError::unsupported(
                format!("the CLIF backend cannot lower {what} yet ({name}.{mname})"),
                scope.def_node.and_then(|n| compiler.hir.span(n)),
            ))
        };
        if scope.runtime_conditional {
            continue;
        }

        let p = &scope.params;
        if let Err(what) = super::emit::check_params(p) {
            return refuse_m(what);
        }
        let layout = super::params::layout_of(p)?;
        let has_blk = scope.needs_block_param();
        let tramp = em
            .module
            .declare_function(
                &em.pkg_symbol(names::class_trampoline_symbol(sym, &format!("__sst_{}_{mname}", m.0))),
                Linkage::Local,
                &params::value_fn_sig(em),
            )
            .map_err(|e| CodegenError::internal(format!("declaring {name}.{mname}: {e}")))?;
        let sig = params::body_sig(em, layout.n_slots, has_blk);
        let body_fn = em
            .module
            .declare_function(
                &em.pkg_symbol(names::class_method_symbol(sym, &format!("__sst_{}_{mname}", m.0))),
                Linkage::Local,
                &sig,
            )
            .map_err(|e| CodegenError::internal(format!("declaring {name}.{mname}: {e}")))?;
        sst.push((owner.0, m.0, mname.clone(), tramp));
        class_methods.push(CmMethodSpec {
            cm_row: false,
            // No `CmRow` is emitted for a super-target-only body, so this
            // is never a lookup key; main is the honest default.
            box_id: 0,
            alias_of: scope.alias_of.clone(),
            defining_class: scope.defining_class,
            lexical_home: scope.lexical_home,
            owner,
            owner_name: name.to_string(),
            name: mname,
            body: scope.body.clone(),
            node: scope.def_node,
            tramp,
            body_fn,
            hir_params: p.clone(),
            has_blk,
            ruby2_keywords: scope.ruby2_keywords,
        });
    }
    Ok(())
}

/// Whether `sid`'s body names an instance variable anywhere -- the one
/// thing that makes a module definition's emitted body carrier-specific.
///
/// A module's own row reads ivars by NAME; an includer's copy reads them by
/// SLOT, and two includers can lay their slots out differently. A body that
/// names none cannot tell the two apart, so it needs only one copy.
///
/// `analyze::collect_ivars` is the same walk `mro` uses to build a class's
/// ivar list, so the two agree by construction. It stops at a nested
/// `class`/`def`, which is right here too: a nested scope is emitted under
/// its own owner and answers this question for itself.
pub(crate) fn scope_names_an_ivar(
    compiler: &crate::compiler::Compiler,
    sid: crate::compiler::ScopeId,
) -> bool {
    !scope_ivar_names(compiler, sid).is_empty()
}

/// Every `@x` a body reads or writes, its parameter defaults included.
fn scope_ivar_names(
    compiler: &crate::compiler::Compiler,
    sid: crate::compiler::ScopeId,
) -> Vec<String> {
    let scope = compiler.scope(sid);
    let mut names = Vec::new();
    for &n in &scope.body {
        crate::analyze::collect_ivars(&compiler.hir, n, &mut names);
    }
    for id in scope.params.default_ids() {
        crate::analyze::collect_ivars(&compiler.hir, id, &mut names);
    }
    names
}

/// The slot `@name` compiles to in a body whose `method_class` is `class`,
/// mirroring `clif::ivars::ivar_slot_of`. `None` is the NAME-keyed capi,
/// which reaches the same storage on any receiver.
fn body_ivar_slot(
    compiler: &crate::compiler::Compiler,
    class: crate::compiler::ClassId,
    name: &str,
) -> Option<usize> {
    let info = compiler.class(class);
    if class == crate::compiler::OBJECT_CLASS || info.is_builtin || info.is_bootstrap {
        return None;
    }
    crate::analyze::class_query::slot_of(compiler, class, name)
}

/// Whether one emitted body can serve `owner` and `carrier` both.
///
/// The body indexes `@x` by the slot it has on the OWNER, so the carrier
/// must agree about every name it touches. A name the owner reaches
/// name-keyed needs no agreement -- that access is receiver-independent.
///
/// `analyze::mro` lays a class out as `ivars(parent) ++ its own new names`,
/// which makes this hold for an ordinary subclass. It is CHECKED rather
/// than assumed because the layout has two documented exceptions: a builtin
/// skips `Object`'s names, and a `Struct`'s members are counted ahead of
/// the ordinary ivars.
fn ivar_slots_agree(
    compiler: &crate::compiler::Compiler,
    owner: crate::compiler::ClassId,
    carrier: crate::compiler::ClassId,
    sid: crate::compiler::ScopeId,
) -> bool {
    scope_ivar_names(compiler, sid).iter().all(|name| {
        match body_ivar_slot(compiler, owner, name) {
            None => true,
            slot => slot == body_ivar_slot(compiler, carrier, name),
        }
    })
}
