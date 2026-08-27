//! Body emission: parameter binding, the method and toplevel body
//! functions, and the captured-cell locals they open and release.

use super::ctx::Fx;
use super::module::Emitter;
use super::{stmt, verify};
use crate::analyze::Analyzed;
use crate::codegen_error::{CResult, CodegenError};
use cranelift_codegen::ir::{self, AbiParam, InstBuilder, MemFlagsData, UserFuncName, types};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{FuncId, Linkage, Module};

pub(crate) struct BodyFnSpec<'a> {
    pub func: FuncId,
    pub owner: zeo_abi::ClassId,
    pub owner_name: &'a str,
    pub name: &'a str,
    pub hir_params: &'a crate::hir::Params,
    pub body: &'a [crate::hir::NodeId],
    pub node: Option<crate::hir::NodeId>,
    pub has_blk: bool,
    pub ruby2_keywords: bool,
    /// A class-method body: `self` is the Class value (frame label
    /// `Owner.name`, ivars are civars).
    pub self_is_class: bool,
    /// A non-method frame label (`<class:Foo>` for a class body); `None`
    /// derives the ordinary `Owner#name`/`Owner.name` label.
    pub label_override: Option<String>,
    /// The caller discards `out` (a class-body fn: the marker call pools
    /// it unread), so the tail runs as a STATEMENT and `out` gets nil --
    /// statement-only shapes (`include`, a nested `class`) may sit last.
    pub discard_value: bool,
    /// Ivars in this body are NAME-KEYED at runtime (a native-backed
    /// owner has no compiled slot layout).
    pub dyn_ivars: bool,
    /// The class the `def` was WRITTEN in (a module method
    /// keeps the module) -- None where `super` refuses.
    pub defining_class: Option<zeo_abi::ClassId>,
    /// The singleton-class SURROGATE this body was lexically written in,
    /// when the `def` sat in a constant-bearing `class << self` body.
    /// Everything LEXICAL resolves through it -- bare constants,
    /// `Module.nesting` -- while the owner keeps dispatch and ivars. See
    /// `Scope::lexical_home`.
    pub lexical_home: Option<zeo_abi::ClassId>,
    /// The name this body was DEFINED under, when an alias reaches it by
    /// another one: `__method__` answers this, `__callee__` the name the
    /// entry carries. `None` when the two are the same.
    pub origin_name: Option<&'a str>,
    /// The `Ruby::Box` this body was compiled in. Every dynamic send,
    /// global and constant owner inside it is keyed by the box, which is
    /// what makes a box's `String#blank?` reachable from the box's own
    /// code and from nowhere else.
    pub box_id: u32,
}

/// A method's frame facts: `(file, label, line, end_line)` -- shared by
/// the body prologue and the trampoline's `ParamDescC`. `class_method`
/// picks ruby's `.` label separator over `#`.
/// CRuby's backtrace label for a class or module BODY frame -- `<class:Foo>`,
/// `<module:M>`, and `singleton class` for a `class << self` body, whose
/// surrogate carries a reserved name ruby cannot spell and must never show.
pub(super) fn body_frame_label(
    compiler: &crate::compiler::Compiler,
    cid: crate::compiler::ClassId,
) -> String {
    if compiler.is_singleton_surrogate(cid) {
        return "singleton class".to_string();
    }
    let kind = if compiler.class(cid).is_module {
        "module"
    } else {
        "class"
    };
    format!("<{kind}:{}>", compiler.leaf_name(cid))
}

pub(super) fn method_frame(
    analyzed: &Analyzed,
    owner_name: &str,
    name: &str,
    node: Option<crate::hir::NodeId>,
    class_method: bool,
) -> (Option<String>, String, u32, u32) {
    let sep = if class_method { "." } else { "#" };
    let label = format!("{owner_name}{sep}{name}");
    let here = node.and_then(|n| crate::analyze::source::source_location(&analyzed.compiler, n));
    let (line, end_line) = match node {
        Some(node) => (
            here.map_or(0, |(_, l)| l),
            crate::analyze::source::source_end_line(&analyzed.compiler, node),
        ),
        None => (0, 0),
    };
    // The body's OWN file, not the program's first: a `require_relative`
    // in a required file resolves against the frame's directory, so a
    // spliced body that named the requiring file would look one directory
    // up.
    let file = here
        .map(|(f, _)| f.to_string())
        .or_else(|| analyzed.compiler.hir.entry_file_name().map(str::to_string));
    (file, label, line, end_line)
}

/// An optional parameter whose binding waits for the frame: `ptr` null =
/// run the default; else copy the given value. A `duplicate` slot (a
/// repeated `_` name) has no storage of its own -- its default still runs
/// for side effects and WRITES the owning local (CRuby compiles a default
/// as an assignment to the local), but a given value is ignored.
struct DeferredOpt {
    name: String,
    default: crate::hir::NodeId,
    ptr: ir::Value,
    duplicate: bool,
}

/// Phase A of param binding (pre-frame, no user code): always-present
/// slots copy into their storage (cells when captured), optionals get
/// nil storage and a [`DeferredOpt`], `&b` binds from the block channel.
/// Only the FIRST slot of a repeated `_` name owns the readable local.
fn bind_param_slots(
    fx: &mut Fx,
    p: &crate::hir::Params,
    entry: &[ir::Value],
    blk_ptr: Option<ir::Value>,
    captured: &crate::compiler::FSet<String>,
) -> Vec<DeferredOpt> {
    let mut bound: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut deferred = Vec::new();
    let mut s = 0usize;
    let always =
        |fx: &mut Fx, bound: &mut std::collections::HashSet<String>, name: &str, slot: usize| {
            let ptr = entry[1 + slot];
            if !bound.insert(name.to_string()) {
                return; // a later duplicate slot binds nothing
            }
            let src = super::operand::Operand::Ptr {
                addr: ptr,
                owned: false,
                tag: super::operand::TagInfo::Unknown,
            };
            if captured.contains(name) {
                let seed = fx.temp_slot();
                let seed_addr = fx.slot_addr(seed, 0);
                super::ownership::write_move_into(fx, &src, seed_addr);
                init_cell_local(fx, name.to_string(), Some(seed_addr));
            } else {
                let ss = fx.new_value_slot();
                let dst = fx.slot_addr(ss, 0);
                super::ownership::write_move_into(fx, &src, dst);
                fx.locals
                    .insert(name.to_string(), super::ctx::Local::Slot(ss));
            }
        };
    let defer = |fx: &mut Fx,
                 bound: &mut std::collections::HashSet<String>,
                 deferred: &mut Vec<DeferredOpt>,
                 name: &str,
                 default: crate::hir::NodeId,
                 slot: usize| {
        let duplicate = !bound.insert(name.to_string());
        if !duplicate {
            if captured.contains(name) {
                init_cell_local(fx, name.to_string(), None);
            } else {
                let ss = fx.new_value_slot();
                fx.locals
                    .insert(name.to_string(), super::ctx::Local::Slot(ss));
            }
        }
        deferred.push(DeferredOpt {
            name: name.to_string(),
            default,
            ptr: entry[1 + slot],
            duplicate,
        });
    };

    for name in &p.required {
        always(fx, &mut bound, name, s);
        s += 1;
    }
    for (name, default) in &p.optional {
        defer(fx, &mut bound, &mut deferred, name, *default, s);
        s += 1;
    }
    // An anonymous `*` has no slot.
    if let Some(Some(name)) = &p.rest {
        always(fx, &mut bound, name, s);
        s += 1;
    }
    for name in &p.post {
        always(fx, &mut bound, name, s);
        s += 1;
    }
    for kw in &p.keywords {
        match kw {
            crate::hir::KeywordParam::Required(name) => always(fx, &mut bound, name, s),
            crate::hir::KeywordParam::Optional(name, default) => {
                defer(fx, &mut bound, &mut deferred, name, *default, s);
            }
        }
        s += 1;
    }
    // An anonymous `**` has no slot.
    if let Some(Some(name)) = &p.keyword_rest {
        always(fx, &mut bound, name, s);
        s += 1;
    }
    let _ = s;
    // `&b`: nil when called blockless (real Ruby), else another reference
    // to the moved-in block (the body's own is still released at exit).
    if let (Some(Some(name)), Some(blk)) = (&p.block, blk_ptr)
        && bound.insert(name.to_string())
    {
        if captured.contains(name) {
            init_cell_local(fx, name.to_string(), None);
        } else {
            let ss = fx.new_value_slot();
            fx.locals
                .insert(name.to_string(), super::ctx::Local::Slot(ss));
        }
        let got =
            fx.b.ins()
                .icmp_imm_u(cranelift_codegen::ir::condcodes::IntCC::NotEqual, blk, 0);
        let yes = fx.b.create_block();
        let join = fx.b.create_block();
        fx.b.ins().brif(got, yes, &[], join, &[]);
        fx.b.switch_to_block(yes);
        let src = super::operand::Operand::Ptr {
            addr: blk,
            owned: false,
            tag: super::operand::TagInfo::Unknown,
        };
        super::ownership::write_local(fx, name, &src);
        fx.b.ins().jump(join, &[]);
        fx.b.switch_to_block(join);
    }
    deferred
}

/// Phase B (the frame exists): each deferred optional either copies its
/// given value or evaluates its default -- user code, in declared order,
/// so a later default reads every earlier binding.
fn bind_deferred(fx: &mut Fx, deferred: &[DeferredOpt]) -> CResult<()> {
    for d in deferred {
        let given = fx.b.create_block();
        let absent = fx.b.create_block();
        let join = fx.b.create_block();
        let nonnull =
            fx.b.ins()
                .icmp_imm_u(cranelift_codegen::ir::condcodes::IntCC::NotEqual, d.ptr, 0);
        fx.b.ins().brif(nonnull, given, &[], absent, &[]);
        fx.b.switch_to_block(given);
        if !d.duplicate {
            let src = super::operand::Operand::Ptr {
                addr: d.ptr,
                owned: false,
                tag: super::operand::TagInfo::Unknown,
            };
            super::ownership::write_local(fx, &d.name, &src);
        }
        fx.b.ins().jump(join, &[]);
        fx.b.switch_to_block(absent);
        let op = super::expr::lower_expr(fx, d.default)?;
        super::ownership::write_local(fx, &d.name, &op);
        fx.b.ins().jump(join, &[]);
        fx.b.switch_to_block(join);
    }
    Ok(())
}

/// One compiled method body: `(self, p1..pn, out) -> i32`. Params are
/// copied into owned slots (the M0 rule -- borrow-through is a perf-pass
/// lever); the tail value moves into `out`; `return` jumps to the shared
/// ok-exit.
pub(super) fn define_method_body(
    em: &mut Emitter,
    analyzed: &Analyzed,
    def: &BodyFnSpec<'_>,
) -> CResult<()> {
    let layout = super::params::layout_of(def.hir_params)?;
    let sig = super::params::body_sig(em, layout.n_slots, def.has_blk);
    let idx = em.next_fn_index();
    // A method materialized onto a carrier keeps its DEFINING class in the
    // frame label: CRuby names where the `def` was written (`M#mixed`,
    // never `Bar#mixed`), read off `scope.defining_class`.
    //
    // A SUPERCLASS is the same rule and used to be excluded, so every
    // backtrace through an inherited method named the receiver's class:
    // `S1#boom` where ruby says `Base#boom`. That also made two carriers'
    // copies of one body differ in nothing but this string.
    // Only a real ANCESTOR renames the label. A `def self.x` written in a
    // `class << self` body has the singleton SURROGATE as its defining
    // class, and ruby still calls that frame `Config.direct` -- the
    // surrogate is an internal name no user can write. It is not in the
    // owner's ancestry, so this test excludes it and the module and
    // superclass cases both keep working.
    let label_owner = match def.defining_class {
        Some(dc)
            if dc != def.owner && analyzed.compiler.class(def.owner).ancestors.contains(&dc) =>
        {
            analyzed.compiler.fq_name(dc)
        }
        _ => def.owner_name.to_string(),
    };
    let (file, label, line, end_line) = method_frame(
        analyzed,
        &label_owner,
        def.name,
        def.node,
        def.self_is_class,
    );
    // A body that came from a literal `define_method(:name) { .. }` rather
    // than a `def`: a BARE `super` is an error in it, a `break` returns, and
    // ruby labels its frame as the BLOCK it is (`block in <class:Named>`),
    // never after the method it installs.
    let define_method_body = def.node.is_some_and(|n| {
        matches!(
            &analyzed.compiler.hir[n],
            crate::hir::HirNode::DefMethod { is_def: false, .. }
        )
    });
    let base = define_method_body
        .then(|| body_frame_label(&analyzed.compiler, def.defining_class.unwrap_or(def.owner)));
    let label = match (def.label_override.clone(), &base) {
        (Some(l), _) => l,
        (None, Some(base)) => format!("block in {base}"),
        (None, None) => label,
    };

    let mut func = ir::Function::with_name_signature(UserFuncName::user(0, idx), sig);
    let cfg = em.module.target_config();
    let mut fbc = FunctionBuilderContext::new();
    let b = FunctionBuilder::new(&mut func, &mut fbc);
    let mut fx = Fx::new(em, analyzed, b, |em, b| {
        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        let rodata_gv = em.module.declare_data_in_func(em.rodata_id, b.func);
        let syms_gv = em.module.declare_data_in_func(em.syms_id, b.func);
        let rodata = b.ins().symbol_value(em.ptr, rodata_gv);
        let syms = b.ins().symbol_value(em.ptr, syms_gv);
        (rodata, syms)
    });
    let entry = fx.b.current_block().expect("entry is current");
    let entry_params: Vec<ir::Value> = fx.b.block_params(entry).to_vec();
    let self_ptr = entry_params[0];
    let out_ptr = *entry_params.last().expect("out is the last param");
    let blk_ptr = def.has_blk.then(|| entry_params[entry_params.len() - 2]);
    fx.self_ptr = Some(self_ptr);
    fx.method_class = Some(def.owner);
    fx.box_id = def.box_id;
    fx.defining_class = def.defining_class;
    fx.lexical_home = def.lexical_home;
    fx.define_method_body = define_method_body;
    fx.method_name = (!def.name.is_empty()).then(|| def.name.to_string());
    fx.method_origin = def.origin_name.map(str::to_string);
    fx.method_params = Some(def.hir_params.clone());
    fx.self_is_class = def.self_is_class;
    fx.dyn_ivars = def.dyn_ivars;
    // A block written inside this body counts from the scope ruby names:
    // the class body a `define_method` sits in, one level up.
    (fx.frame_label, fx.block_depth) = match base {
        Some(base) => (base, 1),
        None => (label.clone(), 0),
    };
    fx.blk_ptr = blk_ptr;
    fx.ruby2_keywords = def.ruby2_keywords;
    let ret_ok = fx.b.create_block();
    fx.ret = Some((out_ptr, ret_ok));
    // The non-local-return home: pushed when a Proc built in this body (or
    // one running under a begin) can aim a `Signal::Return` here.
    let needs_return_catch =
        crate::analyze::captures::body_contains_escaping_return(&analyzed.compiler, def.body)
            || crate::analyze::captures::body_contains_begin(&analyzed.compiler, def.body)
            || crate::analyze::captures::body_contains_runtime_eval(&analyzed.compiler, def.body);

    // Recursion guard BEFORE the frame exists: a failure returns without
    // pops.
    let status = fx.call_status("zeo_rt_stack_check", &[]);
    let early = fx.b.create_block();
    let cont = fx.b.create_block();
    fx.b.ins().brif(status, early, &[], cont, &[]);
    fx.b.switch_to_block(early);
    let one = fx.b.ins().iconst(types::I32, 1);
    fx.b.ins().return_(&[one]);
    fx.b.switch_to_block(cont);
    if needs_return_catch {
        fx.call("zeo_rt_home_push", &[]);
    }

    // What escaping blocks capture becomes a cell instead of a slot -- and
    // so does every local a `binding` taken here would report, because a
    // cell is the only storage a binding can share. `binding_scope_names`
    // adds them to the set and hands back the list the binding reports.
    let mut caps = crate::analyze::captures::collect_escaping_captures(
        &analyzed.compiler,
        def.body,
        def.hir_params,
        Some(def.owner),
    );
    fx.binding_names = crate::analyze::captures::binding_scope_names(
        &analyzed.compiler,
        def.body,
        def.hir_params,
        &mut caps,
        false,
    );
    let captured = caps.locals;
    // Always-present params bind now (no user code); optionals get their
    // storage and defer to after the frame exists (a default is user code
    // that can raise, and CRuby attributes it to the method).
    let deferred = bind_param_slots(&mut fx, def.hir_params, &entry_params, blk_ptr, &captured);
    // The body's other locals, nil-initialized (cells when captured) --
    // including what the DEFAULT expressions themselves assign.
    let mut locals = crate::analyze::local_storage::Locals::default();
    for &stmt in def.body {
        crate::analyze::local_storage::collect_locals(&analyzed.compiler, stmt, &mut locals);
    }
    for id in def.hir_params.default_ids() {
        crate::analyze::local_storage::collect_locals(&analyzed.compiler, id, &mut locals);
    }
    let mut hoisted_names = locals.names().to_vec();
    for (_, group) in &def.hir_params.destructures {
        group.collect_local_names(&mut hoisted_names);
    }
    for name in hoisted_names {
        if fx.locals.contains_key(&name) {
            continue;
        }
        if captured.contains(&name) {
            init_cell_local(&mut fx, name, None);
        } else {
            let ss = fx.new_value_slot();
            fx.locals.insert(name, super::ctx::Local::Slot(ss));
        }
    }

    // `$~` is frame-local: a scope that can touch the family gets its own
    // svar scope, so a match it performs never reaches the caller's `$1`.
    // The svar push must stay between frame push and checkpoint, so only
    // the svar-free common case takes the fused call -- this prologue runs
    // per method CALL.
    let needs_svar = crate::analyze::svars::body_mentions_svars(&analyzed.compiler, def.body);
    if let Some(file) = &file {
        let off = fx.em.intern_rodata(file.as_bytes());
        let label_off = fx.em.intern_rodata(label.as_bytes());
        let file_ptr = fx.rod(off);
        let file_len = fx.b.ins().iconst(fx.em.ptr, file.len() as i64);
        let label_ptr = fx.rod(label_off);
        let label_len = fx.b.ins().iconst(fx.em.ptr, label.len() as i64);
        let line_v = fx.b.ins().iconst(types::I32, i64::from(line));
        let end_v = fx.b.ins().iconst(types::I32, i64::from(end_line));
        let args = [file_ptr, file_len, label_ptr, label_len, line_v, end_v];
        if needs_svar {
            super::frames::fetch_frame_hot(&mut fx);
            super::frames::emit_frame_push(&mut fx, &args);
            fx.call("zeo_rt_svar_scope_push", &[]);
            fx.check_ints();
        } else {
            super::frames::emit_frame_enter(&mut fx, &args);
        }
    } else {
        if needs_svar {
            fx.call("zeo_rt_svar_scope_push", &[]);
        }
        fx.check_ints();
    }

    bind_deferred(&mut fx, &deferred)?;
    // Parenthesized destructuring params replay as the multi-assignments
    // they are, after every slot is bound and before the body runs.
    for (read, group) in &def.hir_params.destructures {
        let op = super::expr::lower_expr(&mut fx, *read)?;
        let tag = op.tag();
        let ptr = super::ownership::borrow_ptr(&mut fx, &op);
        if op.owned() {
            super::ownership::pool_owned(&mut fx, ptr, tag);
        }
        super::multi::lower_multi_group(&mut fx, *read, group, ptr)?;
    }
    // A scope that lexically contains a run-time `eval` publishes what a
    // snippet's `yield`, `block_given?` and bare `super` mean: CRuby reads
    // those off the caller's control frame, and zeo has no equivalent, so
    // the home rides on its own stack for the length of the call. A
    // `define_method` body publishes no `super` target -- a bare `super`
    // is an error in one, and the snippet must say so rather than forward
    // the block's parameters.
    // A CLASS BODY publishes nothing: it can never have a block, and CRuby
    // refuses a `yield` written in an `eval` called from one outright
    // (`Invalid yield`) rather than raising `LocalJumpError`. An empty
    // name is what a class-body spec carries.
    let publishes_eval_home = !def.name.is_empty()
        && crate::analyze::captures::body_contains_runtime_eval(&analyzed.compiler, def.body);
    if publishes_eval_home {
        let params = def.hir_params.clone();
        let (args, kw, unmark) = if define_method_body {
            let null = fx.b.ins().iconst(fx.em.ptr, 0);
            (null, null, false)
        } else {
            super::call::build_zsuper_args(&mut fx, &params)?
        };
        let unmark_v = fx.b.ins().iconst(types::I8, i64::from(unmark));
        let blk = match blk_ptr {
            Some(b) => b,
            None => fx.b.ins().iconst(fx.em.ptr, 0),
        };
        let defining = def.defining_class.unwrap_or(def.owner);
        let dc = fx.b.ins().iconst(types::I32, i64::from(defining.0));
        let sym = fx.sym_id(def.name);
        fx.call("zeo_rt_eval_home_push", &[blk, args, unmark_v, kw, dc, sym]);
    }
    if def.discard_value {
        super::stmt::lower_stmts(&mut fx, def.body)?;
        super::ownership::write_move_into(&mut fx, &super::operand::Operand::Nil, out_ptr);
    } else {
        super::stmt::lower_value_body_into(&mut fx, def.body, out_ptr)?;
    }
    fx.b.ins().jump(ret_ok, &[]);

    let has_frame = file.is_some();
    // ONE epilogue, entered by jump, rather than a copy per exit. The
    // sequence below -- the local releases, the frame pop, the svar/home
    // pops -- is identical at every exit and does not depend on the status,
    // which rides in as the block parameter. Emitting it per exit made it
    // most of a small method's code: the frame pop alone is a gate branch,
    // a bounds test and a pool-watermark divide.
    let epi = fx.b.create_block();
    let epi_status = fx.b.append_block_param(epi, types::I32);
    let goto_epi = |fx: &mut Fx, status: i64| {
        let code = fx.b.ins().iconst(types::I32, status);
        fx.b.ins().jump(epi, &[code.into()]);
    };
    let epilogue = |fx: &mut Fx| {
        fx.b.switch_to_block(epi);
        release_locals(fx);
        if let Some(blk) = blk_ptr {
            // The body owns the moved-in block; a null slot releases as a
            // no-op inside the runtime? No -- guard it.
            let got =
                fx.b.ins()
                    .icmp_imm_u(cranelift_codegen::ir::condcodes::IntCC::NotEqual, blk, 0);
            let rel = fx.b.create_block();
            let cont = fx.b.create_block();
            fx.b.ins().brif(got, rel, &[], cont, &[]);
            fx.b.switch_to_block(rel);
            fx.call("zeo_rt_release", &[blk]);
            fx.b.ins().jump(cont, &[]);
            fx.b.switch_to_block(cont);
        }
        if has_frame {
            super::frames::emit_frame_pop(fx);
        }
        if needs_svar {
            fx.call("zeo_rt_svar_scope_pop", &[]);
        }
        if needs_return_catch {
            fx.call("zeo_rt_home_pop", &[]);
        }
        if publishes_eval_home {
            fx.call("zeo_rt_eval_home_pop", &[]);
        }
        fx.b.ins().return_(&[epi_status]);
    };
    fx.b.switch_to_block(ret_ok);
    goto_epi(&mut fx, 0);
    let land = fx.land;
    fx.b.switch_to_block(land);
    if needs_return_catch {
        // A `Signal::Return` aimed at THIS activation (asked before the
        // home pops) folds into the method's own value.
        let kind = fx.call_status("zeo_rt_signal_kind", &[]);
        let is_ret = fx.b.ins().icmp_imm_u(
            cranelift_codegen::ir::condcodes::IntCC::Equal,
            kind,
            i64::from(zeo_abi::abi::SignalKind::Return as u8),
        );
        let ask = fx.b.create_block();
        let normal = fx.b.create_block();
        fx.b.ins().brif(is_ret, ask, &[], normal, &[]);
        fx.b.switch_to_block(ask);
        let mine = fx.call_status("zeo_rt_return_targets_here", &[]);
        let fold = fx.b.create_block();
        fx.b.ins().brif(mine, fold, &[], normal, &[]);
        fx.b.switch_to_block(fold);
        fx.call("zeo_rt_signal_take", &[out_ptr]);
        fx.b.ins().jump(ret_ok, &[]);
        fx.b.switch_to_block(normal);
        goto_epi(&mut fx, 1);
    } else {
        goto_epi(&mut fx, 1);
    }
    epilogue(&mut fx);

    fx.drain_slot_inits();
    verify::check(&fx, &label)?;
    let Fx { mut b, .. } = fx;
    b.seal_all_blocks();
    b.finalize(cfg);

    em.record_clif(&label, &func);
    em.define(def.func, func, &label, true)?;
    Ok(())
}

/// The compiled `<main>` body, `UnitFn`-shaped: hoisted nil-initialized
/// locals, the frame push, `check_ints`, the statements, then `Nil` out --
/// with the ONE landing block releasing the locals and popping the frame
/// (which drains the release pool) on the signal path.
/// Which top-level scope a body fn is: the program's `<main>`, or one
/// compiled-in load-path file the runtime runs when a `require` names it.
/// A unit IS a top-level scope -- its own file-isolated locals, its own
/// frame -- but none of main's once-per-program installs are its.
pub(super) enum TopScope<'a> {
    Main {
        hoisted: &'a [super::collect::ClassBodyCall],
    },
    Unit {
        index: usize,
        file: String,
    },
}

pub(super) fn define_toplevel(
    em: &mut Emitter,
    analyzed: &Analyzed,
    scope: &TopScope<'_>,
    stmts: &[crate::hir::NodeId],
) -> CResult<FuncId> {
    let mut sig = em.module.make_signature();
    sig.params.push(AbiParam::new(em.ptr));
    sig.returns.push(AbiParam::new(types::I32));
    let (sym, label, frame, idx) = match scope {
        TopScope::Main { .. } => (
            super::names::TOPLEVEL.to_string(),
            "<main>".to_string(),
            analyzed.compiler.hir.entry_file_name().map(str::to_string),
            0,
        ),
        TopScope::Unit { index, file } => (
            format!("zeo_unit_{index}"),
            "<top (required)>".to_string(),
            Some(file.clone()),
            em.next_fn_index(),
        ),
    };
    let func_id = em
        .module
        .declare_function(&sym, Linkage::Local, &sig)
        .map_err(|e| CodegenError::internal(format!("declaring {sym}: {e}")))?;

    let mut locals = crate::analyze::local_storage::Locals::default();
    for &stmt in stmts {
        crate::analyze::local_storage::collect_locals(&analyzed.compiler, stmt, &mut locals);
    }

    let mut func = ir::Function::with_name_signature(UserFuncName::user(0, idx), sig);
    let cfg = em.module.target_config();
    let mut fbc = FunctionBuilderContext::new();
    let b = FunctionBuilder::new(&mut func, &mut fbc);
    let mut fx = Fx::new(em, analyzed, b, |em, b| {
        let entry = b.create_block();
        b.append_block_params_for_function_params(entry);
        b.switch_to_block(entry);
        let rodata_gv = em.module.declare_data_in_func(em.rodata_id, b.func);
        let syms_gv = em.module.declare_data_in_func(em.syms_id, b.func);
        let rodata = b.ins().symbol_value(em.ptr, rodata_gv);
        let syms = b.ins().symbol_value(em.ptr, syms_gv);
        (rodata, syms)
    });
    let out_ptr = {
        let entry = fx.b.current_block().expect("entry is current");
        fx.b.block_params(entry)[0]
    };

    fx.frame_label = label.clone();
    // Hoisted locals: an owned slot each, or a cell when an escaping block
    // captures the name.
    let empty_params = crate::hir::Params::default();
    let mut caps = crate::analyze::captures::collect_escaping_captures(
        &analyzed.compiler,
        stmts,
        &empty_params,
        None,
    );
    // `TOPLEVEL_BINDING` IS the top-level frame's binding, so a program that
    // can read it -- anywhere, including inside a required gem's method
    // (erb's `new_toplevel`) -- deoptimizes the top level to cells exactly as
    // a literal `binding` call there would.
    // A UNIT never carries it: `TOPLEVEL_BINDING` names MAIN's frame, so a
    // program that reads it deoptimizes main, not every file it requires. A
    // unit that calls `binding` itself still deoptimizes -- the call is in
    // its own statements.
    let wants_toplevel_binding = matches!(scope, TopScope::Main { .. })
        && analyzed
            .compiler
            .hir
            .nodes()
            .iter()
            .any(|n| matches!(n, crate::hir::HirNode::ClassRef(c) if c == "TOPLEVEL_BINDING"));
    fx.binding_names = crate::analyze::captures::binding_scope_names(
        &analyzed.compiler,
        stmts,
        &empty_params,
        &mut caps,
        wants_toplevel_binding,
    );
    let captured = caps.locals;
    for name in locals.names().to_vec() {
        if captured.contains(&name) {
            init_cell_local(&mut fx, name, None);
        } else {
            let ss = fx.new_value_slot();
            fx.locals.insert(name, super::ctx::Local::Slot(ss));
        }
    }

    // Frame push and interrupt checkpoint fused into one call when a frame
    // exists.
    if let Some(file) = &frame {
        let off = fx.em.intern_rodata(file.as_bytes());
        let main_off = fx.em.intern_rodata(label.as_bytes());
        let file_ptr = fx.rod(off);
        let file_len = fx.b.ins().iconst(fx.em.ptr, file.len() as i64);
        let label_ptr = fx.rod(main_off);
        let label_len = fx.b.ins().iconst(fx.em.ptr, label.len() as i64);
        let zero = fx.b.ins().iconst(types::I32, 0);
        super::frames::emit_frame_enter(
            &mut fx,
            &[file_ptr, file_len, label_ptr, label_len, zero, zero],
        );
    } else {
        fx.check_ints();
    }

    // The toplevel's `self`: one pooled `main` handle, borrowed by every
    // receiverless direct call.
    let self_ss = fx.temp_slot();
    let self_addr = fx.slot_addr(self_ss, 0);
    fx.call("zeo_rt_main_object", &[self_addr]);
    fx.owned_created += 1;
    super::ownership::pool_owned(
        &mut fx,
        self_addr,
        super::operand::TagInfo::Known(zeo_abi::abi::ValueTag::Object as u8),
    );
    fx.self_ptr = Some(self_addr);

    if let TopScope::Main { hoisted } = scope {
        super::emit::main_installs(&mut fx, analyzed, hoisted)?;
    }
    // Defs registered through the row tables run nothing in statement
    // position (registration precedes the
    // body); a ClassDef marker runs its body site inline.
    let runnable: Vec<crate::hir::NodeId> = stmts
        .iter()
        .copied()
        .filter(|&s| {
            !matches!(
                analyzed.compiler.hir[s],
                crate::hir::HirNode::DefMethod { .. }
            )
        })
        .collect();
    // The top level's frame lives for the whole program, so its temps
    // die at their own statement instead.
    fx.drain_temps = true;
    stmt::lower_stmts(&mut fx, &runnable)?;
    fx.drain_temps = false;

    // Normal exit: release the locals, pop the frame (drains the pool),
    // hand back Nil. ONE epilogue for both exits -- see `define_body`.
    let epi = fx.b.create_block();
    let epi_status = fx.b.append_block_param(epi, types::I32);
    let goto_epi = |fx: &mut Fx, status: i64| {
        let code = fx.b.ins().iconst(types::I32, status);
        fx.b.ins().jump(epi, &[code.into()]);
    };
    let z = fx.b.ins().iconst(types::I64, 0);
    for off in [0, 8, 16] {
        fx.b.ins().store(MemFlagsData::trusted(), z, out_ptr, off);
    }
    goto_epi(&mut fx, 0);
    let land = fx.land;
    fx.b.switch_to_block(land);
    goto_epi(&mut fx, 1);

    fx.b.switch_to_block(epi);
    release_locals(&mut fx);
    if frame.is_some() {
        super::frames::emit_frame_pop(&mut fx);
    }
    fx.b.ins().return_(&[epi_status]);

    fx.drain_slot_inits();
    verify::check(&fx, &sym)?;
    let Fx { mut b, .. } = fx;
    b.seal_all_blocks();
    b.finalize(cfg);

    em.record_clif(&sym, &func);
    em.define(func_id, func, &sym, true)?;
    Ok(func_id)
}

/// A fresh CAPTURED local: an owned cell (seeded from `seed`'s moved
/// value, nil when `None`) whose pointer lives in an 8-byte slot.
pub(crate) fn init_cell_local(fx: &mut Fx, name: String, seed: Option<ir::Value>) {
    let init = match seed {
        Some(p) => p,
        None => fx.b.ins().iconst(fx.em.ptr, 0),
    };
    let cellp = fx.call_status("zeo_rt_cell_new", &[init]);
    let ss = fx.new_cell_slot();
    let dst = fx.slot_addr(ss, 0);
    fx.b.ins().store(MemFlagsData::trusted(), cellp, dst, 0);
    fx.locals
        .insert(name, super::ctx::Local::Cell { ss, owned: true });
}

/// Release every local: slots drop their value, owned cells drop their
/// reference (the proc's copies keep the cell alive).
pub(crate) fn release_locals(fx: &mut Fx) {
    let locals: Vec<super::ctx::Local> = fx.locals.values().copied().collect();
    for l in locals {
        match l {
            super::ctx::Local::Slot(ss) => {
                let addr = fx.slot_addr(ss, 0);
                super::ownership::release_if_heap(fx, addr);
            }
            super::ctx::Local::Cell { ss, owned: true } => {
                let ptr = fx.cell_ptr(ss);
                fx.call("zeo_rt_cell_release", &[ptr]);
            }
            super::ctx::Local::Cell { owned: false, .. } => {}
        }
    }
}
