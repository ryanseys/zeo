//! Emits Rust source for the analyzed program: walks classes/methods/top-level
//! statements, emitting `ruby_class!` macro invocations plus a `fn main()`.
//!
//! Generated Rust is built as a `proc_macro2::TokenStream`, composed via
//! `quote!`, rather than hand-formatted `String`/`format!` text -- each
//! `emit_*` function returns a fragment, spliced into its caller's `quote!`
//! rather than string-interpolated. This is a code-generation *library*
//! choice, not proc-macro infrastructure operating on `zeo` itself:
//! `ruby_class!` (defined in `zeo-rt`) still only expands when the
//! *generated program* is compiled by the real `cargo build` in a temp
//! project, exactly as before. `codegen_to_string` re-parses the finished
//! `TokenStream` with `syn` and formats it with `prettyplease` -- a free
//! correctness net: a codegen bug that produces invalid Rust now fails right
//! here as a clean `Result::Err`, not later as a confusing `cargo build`
//! failure in a temp dir.

mod call;
mod captures;
mod class_query;
mod collections;
mod constfold;
mod exceptions;
mod expr;
mod hoisting;
mod ident;
mod loops;
mod params;
mod patterns;
mod share;
mod stmt;

use quote::{format_ident, quote};

use crate::analyze::Analyzed;
use crate::compiler::{ClassId, Compiler, OBJECT_CLASS};
use crate::types::TyKind;
use ident::safe_ident;
use proc_macro2::TokenStream;
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use syn::Lifetime;

#[derive(Clone)]
struct Ctx<'a> {
    compiler: &'a Compiler,
    /// Which `Ruby::Box` the code currently being emitted is DEFINED in --
    /// the AOT analogue of CRuby's `cme->def->box` stamp (a method resolves
    /// names against its DEFINING box, never its caller's). `0` (the main
    /// box) at the top level: a method body carries its `defining_class`'s
    /// box, a `BoxScope` body overrides it. Consumed by `Ctx::resolve_class`
    /// (and by gvar/dispatch emission).
    box_id: u32,
    /// The RECEIVER's concrete class -- i.e. which `impl` block (generated
    /// Rust struct) this method body is being emitted into. Stays fixed
    /// across nested `super` splices (unlike `defining_class` below), since
    /// `self` is always the SAME concrete instance throughout.
    current_class: Option<ClassId>,
    current_method: Option<String>,
    /// The name the current method was DEFINED under, when it reached
    /// `current_method` through an `alias` (`Scope::alias_of`). `__method__`
    /// answers this one and `__callee__` answers `current_method`, which is
    /// the only place the two differ.
    current_method_origin: Option<String>,
    /// Which class/module's HIR body the CURRENTLY-executing method
    /// actually came from -- equal to `current_class` for an ordinary
    /// own-body method, but set to the true source ancestor while emitting
    /// a materialized (inherited or mixed-in) method, or while inlining a
    /// `super` splice (see `compiler::Scope::defining_class`'s docs). Two
    /// distinct roles read this: `super` resolution
    /// (`codegen::call::emit_super`) searches `current_class`'s
    /// `ancestors` starting AFTER this position, and `@@cvar` ownership
    /// lookup (`codegen::expr::cvar_owner_id`) uses it directly (cvar
    /// ownership is a property of where the code was LEXICALLY written,
    /// like a closure's scope -- not of which concrete receiver ends up
    /// calling it).
    defining_class: Option<ClassId>,
    /// Set while emitting a CLASS method's body (`def self.x`, a `class <<
    /// self` def, or a module function) to the class object that is its
    /// `self` -- `None` in every other context, which is what distinguishes
    /// "self is a class" from "self is an instance" at every use site.
    ///
    /// This is the RECEIVER class, not `defining_class`: a class-level `@x`
    /// is per-class-object storage and is NOT inherited (oracle-verified --
    /// see `zeo_rt::civars`' docs for the `Sub.reg` => nil case that
    /// pins this down), so an inherited class method must read the slot of
    /// whichever class was actually called, not of the one whose body it
    /// was written in. That falls out for free rather than needing a
    /// runtime receiver: `analyze::mro::materialize_class_methods` emits an
    /// inherited class method as a separate copy per subclass, so each copy
    /// has its own, statically-correct id here.
    ///
    /// Deliberately NOT folded into `current_class` (which means "the
    /// concrete struct `self` is an instance of", and drives ivar FIELD
    /// access and Path-1 static dispatch -- neither of which a class object
    /// has).
    class_self: Option<ClassId>,
    /// The enclosing method/top-level scope's per-local static types (see
    /// `analyze::locals`) -- lets operator dispatch resolve `x + y` to
    /// native `Int` arithmetic for locals, not just literal operands.
    /// `Cow` (not a plain `&'a HashMap`) so `with_narrowed_locals` can hand a
    /// single `case/in` ARM's own body an OWNED, narrowed overlay (see that
    /// method's docs) without needing a new `Ctx` field of its own.
    local_types: std::borrow::Cow<'a, HashMap<String, TyKind>>,
    /// Shared by every loop nested inside the same generated `fn`, so each
    /// one gets a function-body-unique label (see `loops::fresh_label`) --
    /// Rust happily lets an inner loop shadow an outer one of the same
    /// label, but `cargo clippy` flags it, and reusing a `Cell` here is far
    /// simpler than threading a counter through every `emit_*` function's
    /// return value.
    label_counter: &'a Cell<u32>,
    /// The nearest enclosing native loop's `(redo_label, outer_label)` pair
    /// -- `None` at the top of a method/closure body, `Some` while emitting
    /// the body of a `While`/`Loop`/`For`/the pre-existing `.times`
    /// block-inlining special case. `break`/`next`/`redo` (see
    /// `codegen::loops`) always target the NEAREST one: Ruby has no labeled
    /// break, so a loop's own body simply shadows this field for itself.
    loop_labels: Option<(Lifetime, Lifetime)>,
    /// Whether this position's `next` must SURFACE its value: true only
    /// inside a VALUE-consuming spliced iterator block (`map`/`select`/
    /// `sum`/...), whose inner redo loop evaluates to the iteration's value
    /// -- `next v` then breaks that inner loop with `v` instead of
    /// continuing the outer label (see `loops::emit_next` and
    /// `call`'s `emit_array_iter_value_splice`). `in_loop` resets it: a
    /// plain loop nested inside the block owns its own `next` again.
    next_yields_value: bool,
    /// A `for`-loop's own index variable's statically-known element type,
    /// active only while emitting THAT loop's body -- overrides whatever
    /// `local_types` (a single flat map covering the WHOLE enclosing scope,
    /// not any particular position within it -- see its own docs) would
    /// otherwise say. `local_types` has no notion of "the type as of this
    /// exact position": it's the map after processing the entire body once,
    /// so a `for`-loop variable introduced fresh, mid-scope, shows up there
    /// as `Poly` (or not at all) even though it's provably `Int` for every
    /// read inside the loop's own body (see `codegen::loops::emit_for`).
    /// Mirrors `loop_labels`' same "child context overrides one field for
    /// this construct's own body" shape.
    for_var_override: Option<(String, TyKind)>,
    /// Enclosing-scope local/parameter names that some escaping block (see
    /// `codegen::captures`) captures -- these get the `Captured`
    /// (`Arc<parking_lot::Mutex<RubyValue>>`) storage class instead of a
    /// plain hoisted `let mut` (see `hoisting::local_storage`), computed ONCE
    /// per method/top-level scope, same lifetime as `local_types`.
    /// `Cow`, not a plain `&`, for the same reason `local_types` is one: a
    /// block's own Ctx must SHADOW these for its own parameter names (see
    /// `in_proc`), which means owning a modified copy.
    captured_locals: std::borrow::Cow<'a, HashSet<String>>,
    /// `Some` exactly in a scope that calls `Kernel#binding`, holding that
    /// scope's own local names in `local_variables` order -- what the
    /// `binding` call site hands the runtime (`codegen::call::emit_binding`),
    /// and the marker that makes cell storage outrank `LocalStorage::Shadowed`
    /// (a Binding shares slots by reference, so an object-typed local can't
    /// stay an unboxed `Arc<Concrete>`). See
    /// `codegen::captures::binding_scope_names`.
    binding_names: Option<std::rc::Rc<Vec<String>>>,
    /// Whether the code being emitted came from an AOT-spliced
    /// `eval("literal")` (`HirNode::Eval`). That snippet was parsed on its
    /// OWN, so prism could not know a bare name is one of the enclosing
    /// scope's locals and handed it over as a vcall; Ruby resolves it as the
    /// local, which `emit_call` does here against `binding_names`.
    in_eval_splice: bool,
    /// The identifier that stands for `self` in THIS position -- ordinarily
    /// the literal `self`, but rebound to a fresh capture-alias identifier
    /// while emitting an escaping block's own body that captured `self`
    /// (`let self = ...;` is illegal Rust -- `self` is only bindable as a
    /// receiver parameter -- so the closure clones into a DIFFERENT name;
    /// see `codegen::call`'s Proc-construction docs). Consulted everywhere
    /// an ivar is read/written (`codegen::expr`'s `IvarRead`/`IvarWrite`)
    /// instead of a hardcoded `self`.
    self_ident: proc_macro2::Ident,
    /// Whether the code currently being emitted is inside a real (escaping)
    /// `Proc` closure's own body, as opposed to an ordinary method body or
    /// an inline-spliced fast-path block (`.times`). Changes two things:
    /// `break`/`next`/`redo` with no enclosing native loop raise a `Signal`
    /// instead of panicking (`codegen::loops`), and a bare `return` raises
    /// `Signal::Return` instead of literally returning (`codegen::expr`) --
    /// see `HirNode::Return`'s docs for why a literal Rust `return` stops
    /// being correct exactly at this boundary.
    in_real_proc: bool,
    /// How many real-Proc closure bodies enclose this position -- 0 in a
    /// method/main body, 1 inside `{ ... }`, 2 inside a block in a block.
    /// Drives the backtrace frame label CRuby gives lexically nested
    /// blocks: `block in X`, `block (2 levels) in X`, ... (see
    /// `frames`-related emission in `emit_proc_or_lambda_value`).
    block_depth: u32,
    /// Whether a `__blk` binding exists LEXICALLY at this position -- a
    /// method body whose signature takes a block parameter, a method-body
    /// lambda closure that named its call-site block param `__blk`, or a
    /// closure that cloned the enclosing `__blk` in. Gates the
    /// block-forwarding capture in `emit_proc_or_lambda_value`: a
    /// top-level/class-body block can scan as a bare block use (bare
    /// `super` forwards the caller's block) with no `__blk` anywhere to
    /// clone. False at top level, class bodies, and `constfold`.
    has_blk_binding: bool,
    /// Whether `self_ident` names a `RubyValue` whose concrete class isn't
    /// statically known, rather than an `Arc<Concrete>`/`self`. True exactly
    /// inside an escaping Proc that captured `self`: such a block's receiver
    /// is a closure PARAMETER (`RProc::with_self`), because `instance_exec`
    /// can run the very same block body under a different one -- so nothing
    /// about the receiver's class is known until the call happens.
    ///
    /// Ivar access consults this (`expr`'s `IvarRead`/`IvarWrite`): a
    /// statically-typed self reads a struct FIELD, a dynamic one goes
    /// through `zeo_rt::ivar_get_dyn`/`ivar_set_dyn`'s name-keyed lookup.
    /// So does implicit-self dispatch (`call`'s `boxed_implicit_self`).
    /// Inline-spliced blocks (`.times` and friends) are NOT affected: they
    /// are not `Proc`s, can't be handed to `instance_exec`, and keep the
    /// static field fast path.
    self_is_dynamic: bool,
    /// Whether a dynamic `self` is nonetheless known to be an instance of a
    /// class that lays `current_class`'s ivars out at the SAME slots -- true
    /// only for a body `codegen::share` emits once for a whole hierarchy.
    ///
    /// [`Ctx::shared_body`] says the same thing more directly; this one is
    /// about where the ivars live.
    ///
    /// Without it a dynamic self reaches ivars by name, a linear scan of the
    /// class's name list. With it the emitted index is the same compile-time
    /// constant the class's own body would have used, because
    /// `analyze::mro` lays slots out parent-first and `share` shares nothing
    /// whose members disagree on the number.
    self_slots: bool,
    /// Whether this body is emitted ONCE for a whole group of classes
    /// (`codegen::share`) rather than per class.
    ///
    /// Two things turn off inside one. A per-site inline cache would serve
    /// every class in the group from one slot and thrash, and -- worse -- the
    /// site INDEX differs between two members, so the group's emissions would
    /// no longer be token-identical and nothing would share at all.
    shared_body: bool,
    /// Where [`Ctx::ask`] records what this emission asked about its receiver
    /// class. `Some` only while `codegen::share` is emitting a candidate body;
    /// the resulting [`class_query::Trace`] is what tells it whether another
    /// class can be served by the same function -- see `class_query`'s docs for
    /// why the questions are recorded rather than listed.
    ///
    /// Shared by reference so a cloned `Ctx` -- every nested block, every
    /// spliced `super` -- writes into the same trace as the body it came from.
    trace: Option<&'a std::cell::RefCell<class_query::Trace>>,
    /// `Some` exactly while emitting the body of a RUNTIME-defined method -- a
    /// `def`/`define_method` installed inside a `Class.new`/`Struct.new`/
    /// `Data.define` block, whose class is minted at runtime and so has no
    /// compile-time `defining_class`. Its presence is what tells
    /// `emit_super` to resolve `super` through the runtime method-frame
    /// stack (`zeo_rt::send_super_dynamic`) rather than the compile-time
    /// ancestor splice; the carried `Params` are the enclosing method's own,
    /// for a bare `super`'s argument forwarding. Propagates through `in_proc`
    /// (a `super` inside a block still targets the enclosing method).
    ///
    /// The enclosing CLASS fields are deliberately NOT cleared when a runtime
    /// method body is nested inside a compile-time one (see `emit_expr`'s
    /// `DefMethod`), so lexical constant resolution in the body still sees the
    /// surrounding module nesting. Sites where a runtime `self` must outrank
    /// that lexical context check `self_is_dynamic` first instead -- see
    /// `boxed_implicit_self` and `IvarRead`.
    runtime_super_params: Option<std::rc::Rc<crate::hir::Params>>,
    /// Whether that runtime method body came from `define_method` rather
    /// than a `def`. A BARE `super` is an error in the first and ordinary in
    /// the second -- see `call::super_calls::emit_super`.
    defined_by_define_method: bool,
    /// The frame-label BASE at the position where a `define_method` was
    /// WRITTEN -- ruby labels its body as the block it literally is
    /// (`block in <class:Named>`, `block (2 levels) in <main>`), never after
    /// the method it installs. Captured by `expr`'s `DefMethod` arm before
    /// `current_method` is overwritten (which would make
    /// `enclosing_frame_label` answer the installed method); cleared for a
    /// real `def`, whose frame IS the method. Read by `procs`' frame-label
    /// emission.
    lexical_frame_label: Option<String>,
}

/// The local-type map a scope emits under, given its `binding_names`. An
/// object-typed local in a `binding` scope loses its static type: it lives in
/// a cell now (`hoisting::local_storage`), and every fast path that type
/// unlocks -- a direct `Klass::m(x)` call, a struct-field ivar read -- needs
/// the unboxed `Arc<Concrete>` the cell no longer holds. Dropping the type
/// routes those through ordinary dynamic dispatch instead, which is what the
/// value in the cell supports.
/// Whether `scope`'s body came from a literal `define_method(:name) { .. }`
/// rather than a `def`. A BARE `super` is an error in the first and
/// ordinary in the second -- see `call::super_calls::emit_super`.
fn scope_is_define_method(compiler: &Compiler, scope: &crate::compiler::Scope) -> bool {
    matches!(
        scope.def_node.map(|n| &compiler.hir[n]),
        Some(crate::hir::HirNode::DefMethod { is_def: false, .. })
    )
}

fn binding_scope_local_types<'a>(
    binding_names: Option<&std::rc::Rc<Vec<String>>>,
    captured: &HashSet<String>,
    types: &'a HashMap<String, TyKind>,
) -> std::borrow::Cow<'a, HashMap<String, TyKind>> {
    let Some(names) = binding_names else {
        return std::borrow::Cow::Borrowed(types);
    };
    let demoted =
        |n: &String| captured.contains(n) && matches!(types.get(n), Some(TyKind::Object(_)));
    if !names.iter().any(demoted) {
        return std::borrow::Cow::Borrowed(types);
    }
    let mut owned = types.clone();
    owned.retain(|n, _| !demoted(n));
    std::borrow::Cow::Owned(owned)
}

impl<'a> Ctx<'a> {
    /// Resolves a class/module NAME as seen from the code currently being
    /// emitted -- the one funnel every codegen name-resolution site goes
    /// through, keyed by the lexical cref chain (derived from
    /// `defining_class`'s `lexical_parent` links, so there's no separate
    /// context field to thread) and this context's box. See
    /// `Compiler::resolve_class` for the resolution order.
    fn resolve_class(&self, name: &str) -> Option<ClassId> {
        self.compiler
            .resolve_class(name, self.cref_chain(), self.box_id)
    }

    /// The lexical scope chain enclosing the current code, outermost first
    /// (`resolve_class` walks it back-to-front, i.e. innermost-outward) --
    /// `Compiler::cref_of`'s rule, which also honors the
    /// qualified-definition cut (see `ClassInfo::qualified_def`). Empty at
    /// the top level. Borrowed from the frozen identity cache: codegen
    /// always runs after `mro::materialize` froze it.
    fn cref_chain(&self) -> &'a [ClassId] {
        self.defining_class
            .map(|c| self.compiler.cref_of_ref(c))
            .unwrap_or(&[])
    }

    /// A child context for a native loop's own body -- see `loop_labels`'s
    /// docs.
    fn in_loop(&self, redo: Lifetime, outer: Lifetime) -> Ctx<'a> {
        Ctx {
            loop_labels: Some((redo, outer)),
            next_yields_value: false,
            ..self.clone()
        }
    }

    /// A child context for an AOT-spliced `eval("literal")` body -- see
    /// `in_eval_splice`'s docs.
    fn in_eval_splice(&self) -> Ctx<'a> {
        Ctx {
            in_eval_splice: true,
            ..self.clone()
        }
    }

    /// A child context for a `BoxScope` body: everything inside
    /// resolves classes/constants/globals against the box -- the AOT
    /// translation of CRuby's loading-box context.
    fn in_box(&self, box_id: u32) -> Ctx<'a> {
        Ctx {
            box_id,
            ..self.clone()
        }
    }

    /// A child context for a `for`-loop's own body -- see
    /// `for_var_override`'s docs.
    fn with_for_var(&self, name: &str, ty: TyKind) -> Ctx<'a> {
        Ctx {
            for_var_override: Some((name.to_string(), ty)),
            ..self.clone()
        }
    }

    /// A child context for ONE `case/in` arm's own guard + body (or a
    /// one-liner `in`/`=>`'s condition), narrowed by `overlay` -- see
    /// `codegen::patterns::collect_narrowing`'s docs for what actually gets
    /// inserted (only builtin, non-`Object` types; a name absent from
    /// `overlay` keeps whatever type it already had). Clones the WHOLE
    /// current map once (cheap -- a compile-time-only cost, not a per-call
    /// runtime one) rather than needing a new `Ctx` field, since `Cow`
    /// already models "borrowed until something needs to own a modified
    /// copy" exactly.
    fn with_narrowed_locals(&self, overlay: HashMap<String, TyKind>) -> Ctx<'a> {
        let mut merged = self.local_types.clone().into_owned();
        merged.extend(overlay);
        Ctx {
            local_types: std::borrow::Cow::Owned(merged),
            ..self.clone()
        }
    }

    /// A child context for a real escaping `Proc` closure's own body -- see
    /// `in_real_proc`/`self_ident`'s docs. `loop_labels`/`for_var_override`
    /// reset to `None`: a `break`/`next`/`redo`/`for`-variable lexically
    /// inside the closure is never targeting a loop OUTSIDE it (a Rust
    /// closure is its own function boundary, unlike inlined splices).
    fn in_proc(&self, needs_self_capture: bool, own_params: &HashSet<String>) -> Ctx<'a> {
        // A block's own PARAMETERS shadow whatever the enclosing scope calls
        // the same name -- they are fresh bindings, and nothing about the
        // outer name applies to them. Both maps must forget those names, or
        // the block's body reads its own parameter through the OUTER name's
        // metadata:
        //   - `local_types`: `b = Builder.new; xs.each { |a, b| ... }` typed
        //     the param `b` as Object(Builder) and emitted a
        //     `Builder::new_handle(b)` box around a plain RubyValue (E0308);
        //   - `captured_locals`: `rescue => e` (captured by some block) made
        //     the param `e` of a LATER `each { |e| }` read as a cell,
        //     emitting `e.lock()` on a plain RubyValue (E0599).
        // Both were live bugs; see the corpus's block_param_shadow family.
        let shadow = |mut c: std::borrow::Cow<'a, HashSet<String>>| {
            if own_params.iter().any(|n| c.contains(n)) {
                c.to_mut().retain(|n| !own_params.contains(n));
            }
            c
        };
        let mut local_types = self.local_types.clone();
        if own_params.iter().any(|n| local_types.contains_key(n)) {
            local_types.to_mut().retain(|n, _| !own_params.contains(n));
        }
        Ctx {
            loop_labels: None,
            next_yields_value: false,
            for_var_override: None,
            self_ident: if needs_self_capture {
                format_ident!("__self")
            } else {
                self.self_ident.clone()
            },
            in_real_proc: true,
            // A captured self arrives as `&RubyValue` (the closure's own
            // first parameter) -- see the field's docs.
            self_is_dynamic: needs_self_capture || self.self_is_dynamic,
            // `instance_exec` can run this very body under a different
            // receiver, so no slot layout is known here even when the
            // enclosing shared body had one.
            self_slots: false,
            // A block INSIDE a shared body is still emitted once for the whole
            // group, so it must stay cache-free too or the group's emissions
            // stop matching.
            shared_body: self.shared_body,
            trace: self.trace,
            captured_locals: shadow(self.captured_locals.clone()),
            local_types,
            block_depth: self.block_depth + 1,
            ..self.clone()
        }
    }

    /// Ask one question about the RECEIVER class, recording it when this
    /// emission is being traced. Every per-class read in the emitter should go
    /// through here: what is not recorded is what `codegen::share` cannot know
    /// two classes disagree about. See [`class_query`].
    fn ask(&self, query: class_query::ClassQuery) -> class_query::Answer {
        let cid = self
            .current_class
            .expect("a class query needs a receiver class");
        let answer = query.answer(self.compiler, cid);
        if let Some(trace) = self.trace {
            trace.borrow_mut().record(query, answer);
        }
        answer
    }

    /// [`Ctx::ask`] where the receiver class may be absent (a class method, the
    /// top level). Nothing untraceable is recorded, because nothing that has no
    /// receiver class can vary with one.
    fn ask_opt(&self, query: class_query::ClassQuery) -> Option<class_query::Answer> {
        self.current_class.is_some().then(|| self.ask(query))
    }

    /// The receiver class and this emission's trace as one value, for the
    /// capture walks -- which run far from a `Ctx` but still ask per-class
    /// questions. See [`class_query::SelfClass`].
    fn self_class(&self) -> class_query::SelfClass<'a> {
        class_query::SelfClass::new(self.current_class, self.trace)
    }

    /// [`Ctx::ask`] from BEFORE a `Ctx` exists -- `codegen::share` has to settle
    /// how a body is emitted (`self_slots`) to build the one it asks with, and
    /// that decision is itself a per-class question the trace must carry.
    fn ask_class(
        compiler: &Compiler,
        cid: ClassId,
        trace: Option<&std::cell::RefCell<class_query::Trace>>,
        query: class_query::ClassQuery,
    ) -> class_query::Answer {
        class_query::SelfClass::new(Some(cid), trace)
            .ask(compiler, query)
            .expect("a receiver class was supplied")
    }
}

/// Wrap a method body that needs a `Signal::Return` catch. `home_push`/
/// `home_pop` bracket the activation so a Proc constructed inside it captures
/// this frame as its non-local-return home (see `zeo_rt::signal`); the pop
/// marks the home dead on EVERY exit (normal or signal), so a later `return`
/// through a Proc whose home has unwound raises `LocalJumpError`. Then the
/// method's own `Signal::Return` folds to a normal value. A method with no
/// escaping block / `begin` catches nothing and pushes no home (a bare-`yield`
/// method must not intercept a `Return` meant for a different frame).
fn wrap_method_return(needs_return_catch: bool, inner: TokenStream) -> TokenStream {
    if needs_return_catch {
        quote! {
            zeo_rt::home_push();
            let __ret = (|| -> Result<zeo_rt::RubyValue, zeo_rt::Signal> { #inner })();
            // Asked BEFORE the pop, while this activation is still the top of
            // the home stack: a `Signal::Return` aimed somewhere else is only
            // passing through and must not be folded into this method's value.
            let __mine = matches!(__ret, Err(zeo_rt::Signal::Return(_)))
                && zeo_rt::return_targets_here();
            zeo_rt::home_pop();
            __ret.or_else(|__e| match __e {
                zeo_rt::Signal::Return(__v) if __mine => Ok(__v),
                __e => Err(__e),
            })
        }
    } else {
        inner
    }
}

/// The `(file name, 1-based line)` of `node`'s span start -- `None` for a
/// synthetic node (the exception prelude, `eval` bodies). Backing for
/// backtrace-frame emission: the file string is baked into the binary and
/// the line comes from the file's prebuilt newline index (`line_at`).
pub(crate) fn source_location(
    compiler: &Compiler,
    node: crate::hir::NodeId,
) -> Option<(&str, u32)> {
    let span = compiler.hir.span(node)?;
    let file = compiler.hir.files.get(span.file.0 as usize)?;
    let upto = (span.start as usize).min(file.source.len());
    // Borrowed, not cloned. This is asked once per emitted statement and once
    // per call site, and `pooled_file` dedups the answer anyway -- so a clone
    // here was a fresh allocation per statement for a string that already
    // lives in the arena and outlives every caller.
    Some((file.name.as_str(), file.line_at(upto as u32)))
}

/// The line `node`'s span ENDS on -- a `def`/`class` node's `end` keyword
/// line, which is what `TracePoint` reports for `:return`/`:end` (0, the
/// no-trace-events marker, when the node is span-less).
pub(crate) fn source_end_line(compiler: &Compiler, node: crate::hir::NodeId) -> u32 {
    let Some(span) = compiler.hir.span(node) else {
        return 0;
    };
    let Some(file) = compiler.hir.files.get(span.file.0 as usize) else {
        return 0;
    };
    let upto = (span.end as usize).min(file.source.len());
    // `line_at`, not a newline count from byte 0 -- the same quadratic its
    // own docs describe, left behind here when `source_location` was
    // converted. Every emitted method asks this once, and the file it scans
    // is the whole SPLICED require graph: 96% of a gem-scale compile's
    // samples landed in this one `filter().count()`.
    file.line_at(upto as u32)
}

/// The backtrace-frame push for one method scope: `Class#method` /
/// `Class.method` labels (CRuby's shapes), the `def` keyword's line as the
/// initial line (what a prologue-raised arity error reports,
/// oracle-verified), the first LOCATED body statement's as fallback. The
/// fallback scans the whole body, not just its head, so the predicate
/// matches `stamp_line`'s exactly: any statement that will stamp a line
/// has a frame of this scope to stamp INTO (a head-synthetic body would
/// otherwise stamp the caller's frame). Empty tokens only for a fully
/// span-less scope (the exception prelude) -- CRuby shows no frames for
/// internal methods either, and such a body never stamps.
///
/// Also pushed synthetically on CALL-SITE arity/keyword raise paths
/// (`params::emit_call_args_to`, the dynamic trampolines): CRuby raises
/// "wrong number of arguments" INSIDE the callee's frame at its def line,
/// so the raise block borrows the same guard the real prologue would push.
pub(crate) fn scope_frame_guard(
    compiler: &Compiler,
    scope: &crate::compiler::Scope,
    class_method: bool,
) -> TokenStream {
    let loc = scope
        .def_node
        .and_then(|n| source_location(compiler, n))
        .or_else(|| {
            scope
                .body
                .iter()
                .find_map(|&n| source_location(compiler, n))
        });
    let Some((file, line)) = loc else {
        return quote! {};
    };
    let sep = if class_method { "." } else { "#" };
    // A `define_method` body is a BLOCK, and ruby labels its frame as one --
    // `block in <class:Named>` -- never after the method it installs. A
    // static `DefMethod` scope always sits directly in a class/module body
    // (a nested one compiles to the runtime path, labeled in `procs`).
    let label = if scope_is_define_method(compiler, scope) {
        format!(
            "block in {}",
            body_frame_label(compiler, scope.defining_class)
        )
    } else {
        format!(
            "{}{sep}{}",
            compiler.fq_name(scope.defining_class),
            scope.name
        )
    };
    // The `def`'s `end` line, `TracePoint`'s `:return` lineno; a scope
    // located only through its body (no `def_node`) stays 0 = untraced.
    let end_line = scope.def_node.map_or(0, |n| source_end_line(compiler, n));
    // A scope that MENTIONS an svar (`$~`/`$1`/`Regexp.last_match`) gets its
    // own frame-local `$~` slot, CRuby's special-variable rule: a callee's
    // match is invisible to the caller. Blocks share their method's (no
    // guard of their own -- see `lastmatch`'s module docs).
    let svar = scope_mentions_svars(compiler, scope).then(|| quote! { , zeo_rt::svar_scope() });
    // The stack probe rides the prologue, INSIDE the guard's initializer so
    // this stays one statement (`zeo_tramp!` splices it as `$frame:stmt`):
    // every compiled method checks its depth against the execution context's
    // floor, making runaway recursion a rescuable SystemStackError instead
    // of a native stack-overflow abort.
    let file = pooled_file(file);
    quote! {
        let __frame = {
            zeo_rt::stack_check()?;
            (zeo_rt::FrameGuard::push(#file, #label, #line, #end_line) #svar)
        };
    }
}

/// [`scope_frame_guard`] for a scope named by id, memoized.
///
/// Same tokens, and the memo is sound because the guard is a function of the
/// scope and `class_method` only -- no caller passes anything else in. What it
/// buys is that the per-class dispatch tables stop re-deriving one answer per
/// inheriting class.
pub(crate) fn cached_frame_guard(
    compiler: &Compiler,
    sid: crate::compiler::ScopeId,
    class_method: bool,
) -> TokenStream {
    if let Some(hit) = FRAME_GUARDS.with_borrow(|c| c.get(&(sid, class_method)).cloned()) {
        return hit;
    }
    let guard = scope_frame_guard(compiler, compiler.scope(sid), class_method);
    FRAME_GUARDS.with_borrow_mut(|c| c.insert((sid, class_method), guard.clone()));
    guard
}

thread_local! {
    /// Cleared with the pools at the head of each `codegen` -- see `take_pools`.
    static FRAME_GUARDS: std::cell::RefCell<
        std::collections::HashMap<(crate::compiler::ScopeId, bool), TokenStream>,
    > = std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Whether `scope`'s body (nested blocks included -- they share the method's
/// svar scope) touches the `$~` family: a `LastMatchRef` read, a `$~` write,
/// or a `Regexp.last_match` call. Decides `scope_frame_guard`'s svar-scope
/// push; a false positive costs one dead scope push, a false negative would
/// leak a match to the caller, so the `last_match` check ignores the
/// receiver.
fn scope_mentions_svars(compiler: &Compiler, scope: &crate::compiler::Scope) -> bool {
    fn walk(hir: &crate::hir::Hir, id: crate::hir::NodeId, found: &mut bool) {
        if *found {
            return;
        }
        match &hir[id] {
            crate::hir::HirNode::LastMatchRef(_) => *found = true,
            crate::hir::HirNode::GlobalWrite(name, _) if name == "$~" => *found = true,
            crate::hir::HirNode::Call { name, .. } if name == "last_match" => *found = true,
            _ => hir[id].for_each_child(&mut |c| walk(hir, c, found)),
        }
    }
    let mut found = false;
    for &n in &scope.body {
        walk(&compiler.hir, n, &mut found);
        if found {
            break;
        }
    }
    found
}

/// The frame label of the scope ENCLOSING the current emission position --
/// what a block nested here is labeled under (`block in <this>`): a
/// method (`Class#m` / `Class.m`), a class body (`<class:Foo>`), or
/// `<main>`. Mirrors CRuby's lexical block-frame naming.
/// CRuby's backtrace label for a class or module BODY frame -- `<class:Foo>`,
/// `<module:M>`, and `singleton class` for a `class << self` body.
///
/// The last of those is spelled by ruby, not derived from a name: zeo homes a
/// singleton body on a surrogate class whose reserved name (`#<Class:self>`)
/// is unwritable and internal, and it must not reach a user's backtrace.
fn body_frame_label(compiler: &Compiler, cid: crate::compiler::ClassId) -> String {
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

fn enclosing_frame_label(cx: &Ctx) -> String {
    if let Some(m) = &cx.current_method {
        let def = cx
            .defining_class
            .or(cx.current_class)
            .or(cx.class_self)
            .unwrap_or(OBJECT_CLASS);
        let fq = cx.compiler.fq_name(def);
        if cx.current_class.is_none() && cx.class_self.is_some() {
            return format!("{fq}.{m}");
        }
        return format!("{fq}#{m}");
    }
    if let Some(c) = cx.class_self.or(cx.current_class) {
        return body_frame_label(cx.compiler, c);
    }
    "<main>".to_string()
}

/// The reflection row for one method scope: what Ruby can ask back about a
/// `def` that its fn pointer can't answer -- the signature
/// (`#arity`/`#parameters`) and the `def` keyword's own line
/// (`#source_location`, and the tail of `#inspect`). One helper for all three
/// emission sites: a user class's own methods, its `def self.x` methods, and
/// the methods a reopened builtin gains. Appends one `zeo_rt::MetaRow` to the
/// program's single `__META_ROWS` static (see `codegen`'s final assembly) --
/// one const row per method, registered in one batch call, instead of a
/// builder chain per method.
fn push_method_meta_row(
    compiler: &Compiler,
    class: ClassId,
    scope: &crate::compiler::Scope,
    class_method: bool,
) {
    let id = class.0;
    let name = &scope.name;
    let ctor = format_ident!("{}", if class_method { "sing" } else { "inst" });
    let entries = param_descriptor_entries(&scope.params);
    let params = (!entries.is_empty()).then(|| quote! { .params(&[#(#entries),*]) });
    let at = scope
        .def_node
        .and_then(|n| source_location(compiler, n))
        .map(|(file, line)| {
            let file = pooled_file(file);
            quote! { .at(#file, #line) }
        });
    let alias = scope
        .alias_of
        .as_ref()
        .map(|original| quote! { .alias(#original) });
    let row = quote! { zeo_rt::MetaRow::#ctor(#id, #name) #params #at #alias };
    POOLS.with(|p| p.borrow_mut().metas.push(row));
}

/// One `__VM_ROWS` row -- see `PoolBuilder::vm_rows`.
fn push_vm_row(target: u32, box_id: u32, key: &str, tramp: TokenStream) {
    POOLS.with_borrow_mut(|p| p.vm_rows.push(quote! { (#target, #box_id, #key, #tramp) }));
}

/// One `__CM_ROWS` row -- see `PoolBuilder::cm_rows`.
fn push_cm_row(id: u32, key: &str, tramp: TokenStream) {
    POOLS.with_borrow_mut(|p| p.cm_rows.push(quote! { (#id, #key, #tramp) }));
}

/// One `__VIS_ROWS` row -- see `PoolBuilder::vis_rows` for the verb scheme.
fn push_vis_row(id: u32, key: &str, verb: u8) {
    POOLS.with_borrow_mut(|p| p.vis_rows.push(quote! { (#id, #key, #verb) }));
}

/// The parameter descriptor entries for one method's `Params`, in Ruby's
/// canonical `#parameters` order (required, optional, rest, post, keywords,
/// keyword-rest, block). Internal destructure-slot names (`__destr_N`) are
/// emitted anonymous, matching CRuby's nameless `[:req]` for a `|(a, b)|`
/// slot.
fn param_descriptor_entries(params: &crate::hir::Params) -> Vec<TokenStream> {
    use crate::hir::KeywordParam;
    fn entry(kind: &str, name: Option<&str>) -> TokenStream {
        let k = format_ident!("{}", kind);
        match name {
            Some(n) if !n.starts_with("__") => {
                quote! { (zeo_rt::ParamKind::#k, Some(#n)) }
            }
            _ => quote! { (zeo_rt::ParamKind::#k, None) },
        }
    }
    let mut out = Vec::new();
    for r in &params.required {
        out.push(entry("Req", Some(r)));
    }
    for (o, _) in &params.optional {
        out.push(entry("Opt", Some(o)));
    }
    // An ANONYMOUS rest/keyrest/block is named for its own sigil: ruby reports
    // `def m(*)` -- and each of `def m(...)`'s three slots -- as `[:rest, :*]`.
    // Only a C function reports a bare `[[:rest]]`, which is what
    // `anonymous_descriptor` still synthesizes. A `__`-prefixed name IS
    // anonymous: it is the internal one lowering gave a bare sigil.
    fn sigil_entry(kind: &str, name: &Option<String>, sigil: &str) -> TokenStream {
        match name {
            Some(n) if !n.starts_with("__") => entry(kind, Some(n)),
            _ => entry(kind, Some(sigil)),
        }
    }
    if let Some(rest) = &params.rest {
        out.push(sigil_entry("Rest", rest, "*"));
    }
    for p in &params.post {
        out.push(entry("Req", Some(p)));
    }
    for kw in &params.keywords {
        match kw {
            KeywordParam::Required(n) => out.push(entry("KeyReq", Some(n))),
            KeywordParam::Optional(n, _) => out.push(entry("Key", Some(n))),
        }
    }
    if let Some(kwrest) = &params.keyword_rest {
        out.push(sigil_entry("KeyRest", kwrest, "**"));
    }
    if let Some(block) = &params.block {
        out.push(sigil_entry("Block", block, "&"));
    }
    out
}

thread_local! {
    static UNSUPPORTED: std::cell::RefCell<Option<(String, Option<crate::hir::Span>)>> =
        const { std::cell::RefCell::new(None) };
}

/// Records a construct codegen can't emit, keeping the FIRST message so the
/// report names the earliest failure rather than the deepest. Emission runs to
/// completion; `codegen_to_string` turns the record into a `CompileError`.
///
/// This exists because a `panic!` here unwinds straight through the test
/// harness, which makes an unsupported construct impossible to check in as an
/// XFAIL repro. Genuine compiler-invariant violations still panic.
pub(crate) fn record_unsupported(message: impl Into<String>) {
    record_unsupported_at(message, None);
}

/// [`record_unsupported`] carrying the span of the node that could not be
/// emitted, so the diagnostic renders the same excerpt the other two stages do.
pub(crate) fn record_unsupported_at(message: impl Into<String>, span: Option<crate::hir::Span>) {
    UNSUPPORTED.with_borrow_mut(|slot| {
        slot.get_or_insert_with(|| (message.into(), span));
    });
}

/// [`record_unsupported`] plus a `nil` stand-in for expression and statement
/// position. The stand-in is never compiled -- `codegen_to_string` fails before
/// the token stream is parsed.
pub(crate) fn unsupported(message: impl Into<String>) -> TokenStream {
    record_unsupported(message);
    quote! { zeo_rt::RubyValue::Nil }
}

/// [`unsupported`] that names where. Prefer it: the message then carries no
/// path of its own, which keeps the committed gem ledger free of one machine's
/// directory layout while still pointing at the line.
pub(crate) fn unsupported_at(
    compiler: &Compiler,
    node: crate::hir::NodeId,
    message: impl Into<String>,
) -> TokenStream {
    record_unsupported_at(message, compiler.hir.span(node));
    quote! { zeo_rt::RubyValue::Nil }
}

fn take_unsupported() -> Option<(String, Option<crate::hir::Span>)> {
    UNSUPPORTED.with_borrow_mut(|slot| slot.take())
}

/// The per-compile literal-pool accumulator (`zeo_rt::SymPool`/`LitPool`):
/// every literal symbol name and frozen string codegen emits registers here,
/// call sites index the pool (`crate::__SYMS.s(4)`), and `codegen` appends
/// the two backing statics after the program tokens. Thread-local for the
/// same reason `UNSUPPORTED` is -- codegen is single-threaded per compile
/// and threading an accumulator through every emit fn would bloat every
/// signature for bookkeeping no reader cares about.
#[derive(Default)]
struct PoolBuilder {
    sym_ix: HashMap<String, usize>,
    syms: Vec<String>,
    lit_ix: HashMap<String, usize>,
    lits: Vec<String>,
    /// Source-file paths named by frame guards (`static __FILES: [&str; n]`),
    /// deduped -- see [`pooled_file`] for why this one matters most.
    file_ix: HashMap<String, usize>,
    files: Vec<String>,
    /// Per-signature `Proc#parameters` tables (`static __PP_N: [ProcParamMeta;
    /// k]`), deduped by rendered signature -- constructing a proc then borrows
    /// one table instead of allocating a `Vec` + interning names per call.
    pp_ix: HashMap<String, usize>,
    pps: Vec<TokenStream>,
    /// Every method's reflection row (`push_method_meta_row`), emitted as the
    /// one `__META_ROWS` static and registered in a single batch call.
    metas: Vec<TokenStream>,
    /// `define_value_method` rows (`(class, box, name, trampoline)`), emitted
    /// as the one `__VM_ROWS` static and applied in a single
    /// `define_value_rows` call after every `register` -- each row's entry
    /// exists by then, and preserving row order preserves last-wins.
    vm_rows: Vec<TokenStream>,
    /// `define_class_method` rows (`__CM_ROWS`), same shape minus the box.
    cm_rows: Vec<TokenStream>,
    /// One `CallSite` inline cache per dynamic call SITE -- never deduped,
    /// because sharing one between two sites is what makes a cache
    /// megamorphic. See `zeo_rt::CallSite`.
    /// One counter per CALLER class, because the caller decides the visibility
    /// question a site asks and is baked into the site's own constructor. A
    /// program has a handful of classes and tens of thousands of call sites, so
    /// grouping this way keeps the emitted arrays O(classes).
    call_sites: std::collections::BTreeMap<u32, usize>,
    /// `mark_private`/`mark_protected`/`mark_public` rows (`__VIS_ROWS`,
    /// verb 0/1/2) -- ONE ordered stream for all three verbs, because a
    /// `public :m` promotion must stay AFTER the private stamp it clears.
    vis_rows: Vec<TokenStream>,
}

thread_local! {
    static POOLS: std::cell::RefCell<PoolBuilder> =
        std::cell::RefCell::new(PoolBuilder::default());
}

/// `crate::__SYMS.s(i)` for a literal symbol name -- the pooled replacement
/// for a per-execution `zeo_rt::Symbol::intern("...")` (a lock + hash even
/// on a hit). `crate::`-qualified so the same tokens work at any module
/// depth (the `__own_`/`__bm_` containers included).
pub(crate) fn pooled_sym(name: &str) -> TokenStream {
    let i = POOLS.with_borrow_mut(|p| {
        if let Some(&i) = p.sym_ix.get(name) {
            return i;
        }
        let i = p.syms.len();
        p.syms.push(name.to_string());
        p.sym_ix.insert(name.to_string(), i);
        i
    });
    let i = proc_macro2::Literal::usize_unsuffixed(i);
    quote! { crate::__SYMS.s(#i) }
}

/// `crate::__LITS.s(i)` -- the pooled interned-frozen-string VALUE, replacing
/// a per-evaluation `RubyValue::Str(intern_frozen(StrBuf::from_utf8(...)))`
/// (which allocated the key `String` and re-hashed even on a hit).
pub(crate) fn pooled_frozen_str(text: &str) -> TokenStream {
    let i = POOLS.with_borrow_mut(|p| {
        if let Some(&i) = p.lit_ix.get(text) {
            return i;
        }
        let i = p.lits.len();
        p.lits.push(text.to_string());
        p.lit_ix.insert(text.to_string(), i);
        i
    });
    let i = proc_macro2::Literal::usize_unsuffixed(i);
    quote! { crate::__LITS.s(#i) }
}

/// `crate::__FILES[i]` -- the pooled source-file PATH a frame guard names.
///
/// Every `FrameGuard::push` carried its file as an inline literal, and a
/// program has orders of magnitude more frames than files: activemodel emitted
/// 40,660 path occurrences drawn from 614 distinct paths, 3.5MB of text for
/// 55KB of content, with the same 96-character path repeated three times inside
/// one 30-line span. Symbols and frozen strings have been pooled for exactly
/// this reason; frames were the one emitter left inlining.
///
/// It also stops the compiling machine's absolute paths from being stamped into
/// the binary tens of thousands of times over. They appear once each now, which
/// is what a backtrace needs and no more.
pub(crate) fn pooled_file(path: &str) -> TokenStream {
    let i = POOLS.with_borrow_mut(|p| {
        if let Some(&i) = p.file_ix.get(path) {
            return i;
        }
        let i = p.files.len();
        p.files.push(path.to_string());
        p.file_ix.insert(path.to_string(), i);
        i
    });
    let i = proc_macro2::Literal::usize_unsuffixed(i);
    quote! { crate::__FILES[#i] }
}

/// A fresh `crate::__CS_N` inline cache for one dynamic call site.
///
/// Deliberately NOT deduped by name: the whole point is that one site tends to
/// see one receiver class, and two sites sharing a cache would each evict the
/// other's answer.
pub(crate) fn pooled_call_site(caller_class: u32) -> TokenStream {
    let i = POOLS.with_borrow_mut(|p| {
        let n = p.call_sites.entry(caller_class).or_default();
        *n += 1;
        *n - 1
    });
    let i = proc_macro2::Literal::usize_unsuffixed(i);
    let arr = call_site_array(caller_class);
    quote! { &crate::#arr[#i] }
}

/// The `__CS_*` array holding every site whose caller class is `caller_class`.
/// `FCALL` gets its own name rather than a number, since `u32::MAX` would
/// otherwise read as a class id.
fn call_site_array(caller_class: u32) -> proc_macro2::Ident {
    match caller_class {
        u32::MAX => format_ident!("__CS_FCALL"),
        c => format_ident!("__CS_{c}"),
    }
}

/// `crate::__PP_N` for one proc signature's `ProcParamMeta` table, deduped
/// across equal signatures. `entries` are the const struct-literal tokens;
/// `key` is any stable rendering of them.
pub(crate) fn pooled_proc_params(key: String, entries: &[TokenStream]) -> TokenStream {
    let i = POOLS.with_borrow_mut(|p| {
        if let Some(&i) = p.pp_ix.get(&key) {
            return i;
        }
        let i = p.pps.len();
        let ident = format_ident!("__PP_{i}");
        let n = entries.len();
        p.pps.push(quote! {
            static #ident: [zeo_rt::ProcParamMeta; #n] = [#(#entries),*];
        });
        p.pp_ix.insert(key, i);
        i
    });
    let ident = format_ident!("__PP_{i}");
    quote! { crate::#ident }
}

fn take_pools() -> PoolBuilder {
    // The frame-guard memo is keyed by `ScopeId`, which only means anything
    // within one compile -- so it is dropped on the same boundary the pools
    // are, and for the same reason.
    FRAME_GUARDS.with_borrow_mut(|c| c.clear());
    POOLS.with_borrow_mut(std::mem::take)
}

thread_local! {
    /// The line-coverage collector -- `Some` only while emitting a program
    /// that required `coverage` (set by `codegen`, drained into the
    /// `coverage_install` table by `coverage_install_tokens`). A program
    /// without the require collects and emits NOTHING, which is the whole
    /// cost model: the instrumentation isn't a checked branch per line, it's
    /// absent.
    static COVERAGE: std::cell::RefCell<Option<CovCollect>> =
        const { std::cell::RefCell::new(None) };
}

#[derive(Default)]
struct CovCollect {
    /// Statement-stamped lines per span file -- every line
    /// `stmt::stamp_line` stamps is a coverable line (CRuby's line events at
    /// the same granularity), collected as emission goes.
    stmt_lines: std::collections::BTreeMap<String, std::collections::BTreeSet<u32>>,
}

pub(crate) fn coverage_active() -> bool {
    COVERAGE.with_borrow(|c| c.is_some())
}

pub(crate) fn coverage_record_stmt(file: &str, line: u32) {
    COVERAGE.with_borrow_mut(|c| {
        if let Some(c) = c {
            c.stmt_lines
                .entry(file.to_string())
                .or_default()
                .insert(line);
        }
    });
}

/// `def` lines per file: definitions the emitted statement stream never
/// stamps -- top-level and class-body `def`s are intercepted at analyze
/// (`process_top_stmt` / `register_body_def_method`) and compiled statically,
/// so no runtime statement passes their line. CRuby reports a definition
/// line's execution count, which for these is exactly once, when the file
/// loads -- `Coverage.result` adds the static 1 for a covered file.
fn coverage_def_lines(
    compiler: &Compiler,
) -> std::collections::BTreeMap<String, std::collections::BTreeSet<u32>> {
    fn walk(
        compiler: &Compiler,
        body: &[crate::hir::NodeId],
        out: &mut std::collections::BTreeMap<String, std::collections::BTreeSet<u32>>,
    ) {
        for &s in body {
            match &compiler.hir[s] {
                crate::hir::HirNode::DefMethod { .. } => {
                    if let Some((file, line)) = source_location(compiler, s) {
                        out.entry(file.to_string()).or_default().insert(line);
                    }
                }
                crate::hir::HirNode::ClassDef { body, .. } => {
                    let body = body.clone();
                    walk(compiler, &body, out);
                }
                _ => {}
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    let programs: Vec<Vec<crate::hir::NodeId>> = compiler
        .hir
        .iter()
        .filter_map(|n| match n {
            crate::hir::HirNode::Program(stmts) => Some(stmts.clone()),
            _ => None,
        })
        .collect();
    for stmts in &programs {
        walk(compiler, stmts, &mut out);
    }
    out
}

/// The `zeo_rt::coverage_install` call for a coverage-activated program:
/// one row per source file -- its total line count (the result array's
/// length), the statement lines collected during emission, and the `def`
/// lines from the HIR walk. `None` when coverage wasn't activated.
fn coverage_install_tokens(compiler: &Compiler) -> Option<TokenStream> {
    let collect = COVERAGE.with_borrow_mut(|c| c.take())?;
    let defs = coverage_def_lines(compiler);
    let mut seen = std::collections::HashSet::new();
    let rows: Vec<TokenStream> = compiler
        .hir
        .files
        .iter()
        .filter(|f| seen.insert(f.name.clone()))
        .filter_map(|f| {
            let stmt = collect.stmt_lines.get(&f.name);
            let def = defs.get(&f.name);
            if stmt.is_none() && def.is_none() {
                return None;
            }
            let name = &f.name;
            let total = f.source.lines().count() as u32;
            let stmts = stmt.into_iter().flatten();
            let defl = def.into_iter().flatten();
            Some(quote! { (#name, #total, &[#(#stmts),*], &[#(#defl),*]) })
        })
        .collect();
    Some(quote! { zeo_rt::coverage_install(&[#(#rows),*]); })
}

/// The assembled program as tokens, with the unsupported-construct record
/// turned into a clean `CompileError` -- the shared front half of both
/// renderers below.
fn codegen_to_tokens(analyzed: &Analyzed) -> Result<TokenStream, crate::diagnostics::CompileError> {
    take_unsupported();
    let tokens = codegen(analyzed);
    if let Some((message, span)) = take_unsupported() {
        return Err(crate::diagnostics::CompileError::codegen_located(
            message,
            span,
            &analyzed.compiler.hir.files,
        ));
    }
    Ok(tokens)
}

/// The BUILD-path renderer: raw `TokenStream` text, no `syn` re-parse and no
/// prettyplease -- rustc is insensitive to formatting, and the re-parse +
/// pretty-print pair dominated emission at gem scale. `ZEO_VALIDATE` restores
/// the in-compiler validation net; without it, a codegen token bug surfaces
/// as a rustc error naming the temp source (and `build_binary` re-parses on
/// failure to say clearly that it is a zeo bug).
pub fn codegen_to_string(analyzed: &Analyzed) -> Result<String, crate::diagnostics::CompileError> {
    let mut out: Vec<u8> = Vec::new();
    codegen_to_writer(analyzed, &mut out)?;
    // Every byte came from a `TokenStream`, which cannot hold invalid UTF-8.
    Ok(String::from_utf8(out).expect("token text is UTF-8 by construction"))
}

/// What an emission produced, for a caller that streamed it somewhere and so
/// never holds the text to measure.
#[derive(Default, Clone, Copy)]
pub struct EmitStats {
    pub bytes: u64,
    pub lines: u64,
}

/// [`codegen_to_string`], streamed. The program is still assembled as one
/// `TokenStream` first (see `codegen`), but the TEXT is never accumulated: it
/// goes to `out` as it is rendered, which is what lets the CLI write straight
/// to a file instead of holding a second whole-program copy in memory.
pub fn codegen_to_writer<W: std::io::Write>(
    analyzed: &Analyzed,
    out: &mut W,
) -> Result<EmitStats, crate::diagnostics::CompileError> {
    let t_tokens = std::time::Instant::now();
    let tokens = codegen_to_tokens(analyzed)?;
    let tokens_ms = t_tokens.elapsed().as_millis();
    if std::env::var_os("ZEO_VALIDATE").is_some() {
        syn::parse2::<syn::File>(tokens.clone()).map_err(|e| {
            crate::diagnostics::CompileError::codegen(format!(
                "codegen produced invalid Rust (this is a zeo bug): {e}"
            ))
        })?;
    }
    let t_emit = std::time::Instant::now();
    let mut counting = Counting {
        inner: out,
        stats: EmitStats::default(),
    };
    let io = (|| -> std::io::Result<()> {
        use std::io::Write as _;
        counting.write_all(b"// Generated by zeo. Do not edit by hand.\n\n")?;
        write_line_broken(tokens, &mut counting)?;
        counting.flush()
    })();
    io.map_err(|e| {
        crate::diagnostics::CompileError::codegen(format!("writing the generated Rust: {e}"))
    })?;
    if crate::timings_enabled() {
        eprintln!(
            "zeo-timings: codegen_tokens={tokens_ms}ms emit={}ms",
            t_emit.elapsed().as_millis(),
        );
    }
    Ok(counting.stats)
}

/// A writer that tallies what passes through it, so `bytes`/`lines` cost one
/// pass rather than a second one over text nobody kept.
struct Counting<W> {
    inner: W,
    stats: EmitStats,
}

impl<W: std::io::Write> std::io::Write for Counting<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.stats.bytes += n as u64;
        self.stats.lines += buf[..n].iter().filter(|&&b| b == b'\n').count() as u64;
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// `TokenStream`'s own `Display` spacing, plus a line break after every `;`
/// and every brace group. `rustc` degrades badly on a multi-megabyte single
/// line -- a gem-scale main can sit in the parser for many minutes -- and
/// `TokenStream::to_string` emits exactly one line. Tokens are atomic -- a string literal is a single
/// token -- so a break BETWEEN tokens is always semantically neutral
/// whitespace, never a meaning change.
fn write_line_broken<W: std::io::Write>(ts: TokenStream, out: &mut W) -> std::io::Result<()> {
    use proc_macro2::{Delimiter, Spacing, TokenTree};
    for tt in ts {
        match tt {
            TokenTree::Group(g) => {
                let delimiter = g.delimiter();
                let (open, close) = match delimiter {
                    Delimiter::Parenthesis => (&b"("[..], &b")"[..]),
                    Delimiter::Brace => (&b"{ "[..], &b"}"[..]),
                    Delimiter::Bracket => (&b"["[..], &b"]"[..]),
                    Delimiter::None => (&b""[..], &b""[..]),
                };
                out.write_all(open)?;
                // `g` is dropped BEFORE the recursion, not after it. proc-macro2
                // holds a group's tokens in a refcounted vector, and iterating a
                // SHARED one has to clone it -- deep-copying every `Ident`'s
                // boxed name and every `Literal`'s string along the way. Letting
                // `g` live across the call kept a second reference alive for the
                // whole subtree, so the walk copied the tree it was consuming.
                let inner = g.stream();
                drop(g);
                write_line_broken(inner, out)?;
                out.write_all(close)?;
                out.write_all(if delimiter == Delimiter::Brace {
                    b"\n"
                } else {
                    b" "
                })?;
            }
            TokenTree::Punct(p) => {
                let mut buf = [0u8; 6];
                out.write_all(p.as_char().encode_utf8(&mut buf).as_bytes())?;
                if p.as_char() == ';' {
                    out.write_all(b"\n")?;
                } else if p.spacing() == Spacing::Alone {
                    out.write_all(b" ")?;
                }
            }
            tt => write!(out, "{tt} ")?,
        }
    }
    Ok(())
}

/// The HUMAN renderer behind `--dump=rust`: the historical `syn` round-trip (a free
/// validity net) plus prettyplease formatting.
pub fn codegen_to_string_pretty(
    analyzed: &Analyzed,
) -> Result<String, crate::diagnostics::CompileError> {
    let t_tokens = std::time::Instant::now();
    let tokens = codegen_to_tokens(analyzed)?;
    let tokens_ms = t_tokens.elapsed().as_millis();
    let t_parse = std::time::Instant::now();
    let file: syn::File = syn::parse2(tokens).map_err(|e| {
        crate::diagnostics::CompileError::codegen(format!(
            "codegen produced invalid Rust (this is a zeo bug): {e}"
        ))
    })?;
    let parse_ms = t_parse.elapsed().as_millis();
    let t_format = std::time::Instant::now();
    let out = format!(
        "// Generated by zeo. Do not edit by hand.\n\n{}",
        prettyplease::unparse(&file)
    );
    if crate::timings_enabled() {
        eprintln!(
            "zeo-timings: codegen_tokens={tokens_ms}ms syn_parse={parse_ms}ms format={}ms",
            t_format.elapsed().as_millis(),
        );
    }
    Ok(out)
}

fn codegen(analyzed: &Analyzed) -> TokenStream {
    take_pools(); // drop any accumulation a prior errored compile left behind
    let compiler = &analyzed.compiler;

    // Line coverage collects only for a program that required `coverage` --
    // see the `COVERAGE` thread-local's docs.
    COVERAGE.set(
        compiler
            .hir
            .activated_features
            .contains("coverage")
            .then(CovCollect::default),
    );

    // `Object` (index 0, built into `zeo-rt`), every MODULE, and every
    // reserved BUILT-IN placeholder (`Integer`/`Array`/etc. -- see
    // `compiler::BUILTIN_CLASSES`) never get a generated Rust struct/
    // `impl RubyObject`/`ClassRegistry` entry via `ruby_class!` at all -- a
    // module's methods only ever manifest indirectly, MATERIALIZED onto
    // whatever includes/prepends/extends it (see `ruby_class!`'s docs), and
    // a built-in type's runtime representation
    // already IS a `RubyValue` variant, needing no separate struct (see
    // `builtin_registrations` below for how it still gets a `ClassRegistry`
    // entry so `is_a?`/`kind_of?` resolve correctly against it -- its
    // `methods` table stays empty, though, since built-in methods dispatch
    // via hardcoded codegen paths rather than the dynamic registry, so
    // `respond_to?` against one always reports `false`, a separate,
    // pre-existing-shaped scope-cut, not a regression this introduces).
    // BOOTSTRAP classes (the built-in exceptions) are excluded too: their
    // structs/impls/registration now live once in `zeo-rt`, installed by
    // `ClassRegistry::with_core` (see `main` below). The compiler still keeps
    // their HIR for name resolution, `super` inlining, and materializing user
    // subclasses -- it just doesn't EMIT them into every program.
    // One body per `def`, not one per class that inherited it -- see
    // `codegen::share`. Planned BEFORE the classes are emitted, because a class
    // whose method is shared emits a forwarding line in place of the body.
    let shared = share::SharedBodies::plan(compiler);
    let shared_container = shared.container().cloned();
    let classes = compiler
        .classes
        .iter()
        .enumerate()
        .filter(|&(idx, _)| compiler.has_generated_struct(ClassId(idx as u32)))
        .map(|(idx, _)| emit_class(compiler, &shared, ClassId(idx as u32)))
        // Eager: the coverage collector (fed by every body emission) is
        // drained before the final `quote!` would consume a lazy iterator.
        .collect::<Vec<_>>();

    // Class methods (`def self.x`, and instance methods pulled in via
    // `extend`) get their own container -- an ADDITIONAL plain `impl` block
    // for a class (Rust allows more than one `impl Type { }` for the same
    // type, no conflict with `ruby_class!`'s own), or a `pub mod` of free
    // functions for a module (which has no struct to attach an `impl` to at
    // all). Emitted for every class/module with any (a module can have
    // `class_methods` of its own too -- "module functions", e.g. `Math.sqrt`).
    let class_method_containers = compiler
        .classes
        .iter()
        .enumerate()
        .filter(|&(idx, class)| {
            idx != 0 && !class.is_builtin && !class.is_bootstrap && !class.class_methods.is_empty()
        })
        .map(|(idx, _)| emit_class_methods(compiler, ClassId(idx as u32)))
        // Eager -- see `classes` above.
        .collect::<Vec<_>>();

    // A REOPENED builtin class: one `pub mod __bm_<Name>`
    // container per builtin that gained any methods, holding its instance
    // methods as free functions (`__self: RubyValue` receiver) and its
    // `def self.x` class methods together (which is why the builtin case is
    // excluded from `class_method_containers` above -- two `mod` items with
    // one name would be a Rust E0428). See `emit_builtin_reopen`'s docs.
    let builtin_reopens = compiler
        .classes
        .iter()
        .enumerate()
        .filter(|&(idx, class)| {
            // `Object` (idx 0) rides the same machinery since top-level
            // `def` support: its methods emit into `__bm_Object` and
            // register as value methods, dispatched on the `main` object
            // at top-level call sites.
            (class.is_builtin || idx == 0)
                && (!class.methods.is_empty() || !class.class_methods.is_empty())
        })
        .map(|(idx, _)| emit_builtin_reopen(compiler, &shared, ClassId(idx as u32)))
        // Eager -- see `classes` above.
        .collect::<Vec<_>>();

    // Class-body statements run at their DOCUMENT position: a site whose
    // `ClassDef` marker is reachable from `main_statements` (directly, via
    // a `BoxScope`, or nested inside another reachable site) emits inline
    // there (`codegen::stmt`'s `ClassDef` arm) -- real Ruby's "a class body
    // runs where it appears, re-running per reopen". Everything else --
    // the prelude's bootstrap bodies and the synthetic `None`-marker
    // registrations -- keeps the hoisted splice at the head of
    // `run_main`'s fallible closure (where `?` propagates as a Signal), so
    // no recorded statement can ever be silently dropped.
    // Units count as top level too. A class written inside one is written
    // THERE, so it has to be emitted there -- walking only `main_statements`
    // left every unit's classes unmarked, so they were hoisted to the head of
    // `run_main` instead, away from the file-level locals they read.
    // activesupport's `duplicable.rb` is the case: `unless
    // methods_are_duplicable; class Method; ... end` hoisted into `main` while
    // `methods_are_duplicable` stayed a local of `__unit_2`.
    let mut top_level_statements = analyzed.main_statements.clone();
    for (_, _, stmts) in &analyzed.feature_units {
        top_level_statements.extend(stmts.iter().copied());
    }
    let inline_markers = inline_class_markers(compiler, &top_level_statements);
    // Every hoisted body is lifted to its own `fn` -- see
    // `emit_class_body_site_lifted`. The counter names them; the items land
    // beside `__unit_N` at the top level, the calls where the body used to be.
    let class_body_fns: RefCell<Vec<TokenStream>> = RefCell::new(Vec::new());
    let hoisted_sites_for = |cid: ClassId| {
        let inline_markers = &inline_markers;
        compiler
            .class_body_sites
            .iter()
            .filter(move |s| {
                s.class == cid
                    && !s.stmts.is_empty()
                    && !s.def_node.is_some_and(|n| inline_markers.contains(&n))
            })
            // Hoisted, not inline: these run at the head of `run_main`,
            // ahead of every top-level statement, so no enclosing local
            // exists for them to read.
            .map(|s| {
                let mut fns = class_body_fns.borrow_mut();
                let (item, call) = emit_class_body_site_lifted(
                    compiler,
                    s,
                    &HashSet::new(),
                    Some(fns.len() as u32),
                    BodyValue::Discard,
                );
                fns.push(item);
                call
            })
            // Eager, like `classes` above -- a lazy iterator would hold the
            // `class_body_fns` borrow across the caller's own work.
            .collect::<Vec<_>>()
    };
    let mut user_class_bodies: Vec<TokenStream> = Vec::new();
    let mut builtin_class_bodies: Vec<TokenStream> = Vec::new();
    // Shadowed extend-copy containers (singleton-chain super targets) --
    // spliced at the top level next to the own-method bridge containers.
    let mut sst_containers: Vec<TokenStream> = Vec::new();

    let mut registrations: Vec<TokenStream> = Vec::new();
    for (idx, class) in compiler.classes.iter().enumerate() {
        // BOOTSTRAP classes are registered by `ClassRegistry::with_core`, not
        // per-program -- skip their whole registration/metadata block here.
        if idx == 0 || class.is_builtin || class.is_bootstrap {
            continue;
        }
        let register = if class.is_module {
            // A module has no generated struct/`__register`, but it
            // still needs a registry entry so its
            // first-class value answers `name`/`.class`/`ancestors`
            // and `puts M` prints its name. No constructor: `M.new`
            // is a real NoMethodError (see `send_value`'s Class arm).
            let id = idx as u32;
            let fq_name = compiler.fq_name(ClassId(id));
            let ancestor_ids = compiler.class(ClassId(id)).ancestors.iter().map(|a| a.0);
            quote! {
                __registry.register(
                    zeo_rt::ClassId(#id),
                    #fq_name,
                    true,
                    vec![#(zeo_rt::ClassId(#ancestor_ids)),*],
                    None,
                );
            }
        } else if compiler.is_exception_backed(ClassId(idx as u32)) {
            // A user `class MyErr < StandardError`: no generated struct --
            // its instances are the native `RubyException`. Install the runtime
            // entry + native `Exception` default method set on its id; the
            // subclass's own `def`s then `define_method` OVER these as deltas
            // (see the exception-delta loop below). The full linearized
            // `ancestors` let the runtime decide `carries_result`
            // (StopIteration descendants) exactly as the built-in tree does.
            let id = idx as u32;
            let fq_name = compiler.fq_name(ClassId(id));
            let ancestor_ids = compiler.class(ClassId(id)).ancestors.iter().map(|a| a.0);
            quote! {
                zeo_rt::register_exception_subclass(
                    &mut __registry,
                    zeo_rt::ClassId(#id),
                    #fq_name,
                    vec![#(zeo_rt::ClassId(#ancestor_ids)),*],
                );
            }
        } else if compiler.is_value_subclass(ClassId(idx as u32)) {
            // A user `class Stack < Array`: no generated struct -- its
            // instances are the native `ValueSubclass` wrapping an `Array`/
            // `String`/`Hash` payload. Register the runtime entry + the shared
            // `value_subclass_construct`; inherited builtin methods come via the
            // `send_in` payload bridge, the subclass's own `def`s via deltas.
            let id = idx as u32;
            let fq_name = compiler.fq_name(ClassId(id));
            let ancestor_ids = compiler.class(ClassId(id)).ancestors.iter().map(|a| a.0);
            quote! {
                zeo_rt::register_value_subclass(
                    &mut __registry,
                    zeo_rt::ClassId(#id),
                    #fq_name,
                    vec![#(zeo_rt::ClassId(#ancestor_ids)),*],
                );
            }
        } else if compiler.is_date_subclass(ClassId(idx as u32))
            || compiler.is_proc_subclass(ClassId(idx as u32))
        {
            // A user `class DateTimeWithOffset < DateTime` or `class P < Proc`:
            // no generated struct -- the root's own rows already tag what they
            // build with the receiver class, so the subclass just has to be
            // passed along.
            let id = idx as u32;
            let fq_name = compiler.fq_name(ClassId(id));
            let ancestor_ids = compiler.class(ClassId(id)).ancestors.iter().map(|a| a.0);
            quote! {
                zeo_rt::register_recv_honouring_subclass(
                    &mut __registry,
                    zeo_rt::ClassId(#id),
                    #fq_name,
                    vec![#(zeo_rt::ClassId(#ancestor_ids)),*],
                );
            }
        } else if compiler.is_weakmap_subclass(ClassId(idx as u32)) {
            // A user `class WeakSet < ObjectSpace::WeakMap`: no generated
            // struct -- its instances ARE the native `WeakMap`, built by the
            // root's own constructor from the receiver class. Its `def`s
            // arrive as `RubyValue`-self deltas.
            let id = idx as u32;
            let fq_name = compiler.fq_name(ClassId(id));
            let ancestor_ids = compiler.class(ClassId(id)).ancestors.iter().map(|a| a.0);
            quote! {
                zeo_rt::register_weakmap_subclass(
                    &mut __registry,
                    zeo_rt::ClassId(#id),
                    #fq_name,
                    vec![#(zeo_rt::ClassId(#ancestor_ids)),*],
                );
            }
        } else if compiler.is_weakref_subclass(ClassId(idx as u32)) {
            // A user `class Ref < WeakRef`: the same shape one root over --
            // its instances ARE the native delegator, and the delegation rows
            // reach it through the ancestry.
            let id = idx as u32;
            let fq_name = compiler.fq_name(ClassId(id));
            let ancestor_ids = compiler.class(ClassId(id)).ancestors.iter().map(|a| a.0);
            quote! {
                zeo_rt::register_weakref_subclass(
                    &mut __registry,
                    zeo_rt::ClassId(#id),
                    #fq_name,
                    vec![#(zeo_rt::ClassId(#ancestor_ids)),*],
                );
            }
        } else if compiler.is_module_subclass(ClassId(idx as u32)) {
            // A user `class X < Module`: no generated struct -- its instances
            // are real runtime MODULE ids tagged as belonging to X, so
            // `include X.new(...)`, `Module#===` and constant lookup all keep
            // working on them. Register the runtime entry + the shared
            // `module_subclass_construct`; X's own `def`s arrive as
            // `RubyValue`-self deltas (`is_native_backed` covers this shape).
            let id = idx as u32;
            let fq_name = compiler.fq_name(ClassId(id));
            let ancestor_ids = compiler.class(ClassId(id)).ancestors.iter().map(|a| a.0);
            quote! {
                zeo_rt::register_module_subclass(
                    &mut __registry,
                    zeo_rt::ClassId(#id),
                    #fq_name,
                    vec![#(zeo_rt::ClassId(#ancestor_ids)),*],
                );
            }
        } else if compiler.is_immediate_subclass(ClassId(idx as u32)) {
            // A user `class MyInt < Integer`: allowed as a DEFINITION but
            // has NO instances. Register just the name + ancestors with NO
            // constructor -- so `MyInt.ancestors`/`superclass`/`is_a?` resolve,
            // while `MyInt.new` dynamically hits `Class#new`'s "no constructor"
            // arm and raises CRuby's `NoMethodError: undefined method 'new' for
            // class MyInt`. No struct, no methods (nothing can be an instance).
            let id = idx as u32;
            let fq_name = compiler.fq_name(ClassId(id));
            let ancestor_ids = compiler.class(ClassId(id)).ancestors.iter().map(|a| a.0);
            quote! {
                __registry.register(
                    zeo_rt::ClassId(#id),
                    #fq_name,
                    false,
                    vec![#(zeo_rt::ClassId(#ancestor_ids)),*],
                    None,
                );
            }
        } else {
            let ident = ident::class_ident(compiler, ClassId(idx as u32));
            quote! { #ident::__register(&mut __registry); }
        };
        registrations.push(register);
        // A COMPILED `Struct`: its member list, so `Struct`'s one shared
        // protocol (`to_a`, `[]`, `==`, `each`, `dig`, `inspect`, `Marshal`)
        // finds it by MRO and reaches the members by index, exactly as it does
        // for a runtime-minted one. Only the accessors differ, in being fields
        // rather than overlay closures.
        let members = &compiler.class(ClassId(idx as u32)).hidden_ivars;
        if !members.is_empty() {
            let id = idx as u32;
            registrations.push(quote! {
                zeo_rt::register_compiled_struct(
                    zeo_rt::ClassId(#id), &[#(#members),*], false, None,
                );
            });
        }
        // Each PRIVATE method materialized onto this class -- its own
        // `private def x`, plus every top-level `def` (a private method of
        // Object, which materialization copies onto every class) -- is
        // recorded so `respond_to?` skips it. A `__VIS_ROWS` row rather than
        // a call inside `ruby_class!`, which has no visibility channel of
        // its own. See `ClassRegistry::mark_visibility_rows`.
        let id = idx as u32;
        for entry in compiler.methods_of(ClassId(id)) {
            let key = compiler.names.str(entry.name);
            match entry.visibility {
                crate::hir::Visibility::Private => push_vis_row(id, key, 0),
                // A protected method is recorded so the `protected_*` reflection
                // and `protected_method_defined?` can report it (and `public_*`
                // exclude it).
                crate::hir::Visibility::Protected => push_vis_row(id, key, 1),
                crate::hir::Visibility::Public => {}
            }
        }
        // A `private`/`public`/`protected :m` re-declaring an INHERITED method's
        // visibility overrides the materialized stamp above -- rows appended
        // after the loop so the override wins (`mark_visibility_rows` applies
        // in order). `mark_public` clears any private/protected mark,
        // promoting the method.
        for (name, vis) in &compiler.class(ClassId(id)).visibility_overrides {
            let verb = match vis {
                crate::hir::Visibility::Private => 0,
                crate::hir::Visibility::Protected => 1,
                crate::hir::Visibility::Public => 2,
            };
            push_vis_row(id, name, verb);
        }
        // The CLASS-method half (verb 3/4): `private_class_method`, either on a
        // `def self.x` in this body or naming one this class inherits.
        for entry in compiler.class_methods_of(ClassId(id)) {
            if entry.visibility == crate::hir::Visibility::Private {
                push_vis_row(id, compiler.names.str(entry.name), 3);
            }
        }
        for (name, vis) in &compiler.class(ClassId(id)).class_visibility_overrides {
            let verb = if *vis == crate::hir::Visibility::Private {
                3
            } else {
                4
            };
            push_vis_row(id, name, verb);
        }
        // Each method DEFINED DIRECTLY on this class (not materialized from an
        // ancestor) is recorded so `instance_methods(false)`/`methods(false)`
        // can report own methods only -- the materialized `methods` list above
        // flattens inheritance in. Sorted for stable generated source.
        let mut own: Vec<&str> = compiler
            .class(ClassId(id))
            .own_methods
            .iter()
            .map(|&sid| compiler.scope(sid).name.as_str())
            // A method this class only RE-SCOPED (`private :inherited_method`)
            // is its own too -- ruby plants a real entry for it, which is what
            // makes it answer `private_instance_methods(false)` here while
            // still running the ancestor's body. See `MethodEntry::zsuper`.
            .chain(
                compiler
                    .methods_of(ClassId(id))
                    .iter()
                    .filter(|e| e.zsuper)
                    .map(|e| compiler.names.str(e.name)),
            )
            .collect();
        own.sort();
        if !own.is_empty() {
            registrations.push(quote! {
                __registry.mark_own_rows(zeo_rt::ClassId(#id), &[#(#own),*]);
            });
        }
        // What each own method's fn pointer can't answer: its signature and
        // its `def` line, baked for `Method`/`UnboundMethod` reflection.
        for &sid in &compiler.class(ClassId(id)).own_methods {
            let scope = compiler.scope(sid);
            push_method_meta_row(compiler, ClassId(id), scope, false);
        }
        // A re-scoped inherited method reflects on THIS class -- ruby answers
        // the subclass for `instance_method(:x).owner` after `private :x`,
        // while still running the ancestor's body.
        for entry in compiler.methods_of(ClassId(id)).iter().filter(|e| e.zsuper) {
            push_method_meta_row(compiler, ClassId(id), compiler.scope(entry.def), false);
        }
        // Every `undef name` in this class's body is recorded so
        // `respond_to?` stops its ancestor walk here -- dispatch itself
        // needs nothing, since `mro::materialize_methods` already left the
        // name out of this class's table. See `ClassEntry::undefined_methods`.
        // Sorted: a HashSet has no stable order, and generated source should
        // not vary between compiles of the same program.
        // `private_constant :A` -- the listing half. The `M::A` reference that
        // must raise is rejected at compile time (`emit_const_read`), so this
        // only keeps `Module#constants` and `defined?` honest.
        let priv_consts: Vec<&str> = compiler
            .class(ClassId(id))
            .private_constants
            .iter()
            .map(String::as_str)
            .collect();
        if !priv_consts.is_empty() {
            registrations.push(quote! {
                zeo_rt::const_set_private(#id, &[#(#priv_consts),*], true);
            });
        }
        // `class C; extend M; end` puts M on C's SINGLETON chain, which the
        // linearized `ancestors` above deliberately excludes -- so it is
        // registered separately, and answers `C.is_a?(M)` and
        // `C.singleton_class.ancestors`.
        let extend_ids: Vec<u32> = compiler
            .class(ClassId(id))
            .extends
            .iter()
            .map(|m| m.0)
            .collect();
        if !extend_ids.is_empty() {
            registrations.push(quote! {
                __registry.register_extends(
                    zeo_rt::ClassId(#id),
                    vec![#(zeo_rt::ClassId(#extend_ids)),*],
                );
            });
        }
        // A `refine` holder is a module in every respect but one: its own
        // `.class` is `Refinement`, which is what a refined `Method#owner`
        // reports.
        if let Some((module, target)) = compiler.refinement_of(ClassId(id)) {
            let (module, target) = (module.0, target.0);
            registrations.push(quote! {
                __registry.mark_refinement(
                    zeo_rt::ClassId(#id),
                    zeo_rt::ClassId(#module),
                    zeo_rt::ClassId(#target),
                );
            });
        }
        let mut undefined: Vec<&String> = compiler.class(ClassId(id)).undefined.iter().collect();
        undefined.sort();
        for key in undefined {
            registrations.push(quote! {
                __registry.mark_undefined(
                    zeo_rt::ClassId(#id),
                    zeo_rt::Symbol::intern(#key),
                );
            });
        }
        // Aliases of inherited BUILTIN methods (`alias_method :raise!,
        // :raise` -- see `ClassInfo::builtin_aliases`): name-indirection
        // rows the send miss paths rewrite through. Subclasses need no
        // copy -- the runtime probe walks the MRO. `validate_aliases`
        // (emitted at the head of `run_main`'s closure) raises `NameError`
        // at startup for a source that resolves nowhere.
        for (new, old) in &compiler.class(ClassId(id)).builtin_aliases {
            registrations.push(quote! {
                __registry.register_alias(zeo_rt::ClassId(#id), #new, #old);
            });
        }
        // The same rows for an alias written inside `class << self` whose
        // source is a builtin CLASS method -- `alias [] new`. They land in the
        // singleton table, which is the one a class-OBJECT receiver consults.
        for (new, old) in &compiler.class(ClassId(id)).class_aliases {
            registrations.push(quote! {
                __registry.register_class_alias(zeo_rt::ClassId(#id), #new, #old);
            });
        }
        // Every `def self.x` also registers for DYNAMIC dispatch, so a class
        // held in a variable can be sent to (`handler = H1; handler.run(...)`
        // -- the receiver isn't a literal constant, so codegen can't emit a
        // direct `H1::__cm_run(...)`). A statically-resolvable `H1.run(...)`
        // still takes the direct path and never touches this table.
        // Registered here rather than in `ruby_class!` for the same reason
        // `mark_private` is: the macro has no channel for it, and a MODULE
        // (`def self.x` on a module -- `Math.sqrt`-shaped) has no generated
        // `__register` at all, yet needs its class methods reachable too.
        for entry in compiler.class_methods_of(ClassId(id)) {
            let scope = compiler.scope(entry.def);
            let container = ident::class_ident(compiler, ClassId(id));
            let method_ident = ident::class_method_ident(&scope.name);
            let fn_path = quote! { #container::#method_ident };
            let tramp = params::emit_value_trampoline(
                &fn_path,
                &scope.name,
                &scope.params,
                scope.needs_block_param(),
                // A class method takes no receiver parameter -- see `RecvMode`.
                params::RecvMode::Drop,
                &scope_frame_guard(compiler, scope, true),
            );
            // Keyed on the real Ruby name, not the mangled Rust ident.
            push_cm_row(id, &scope.name, tramp);
        }
        // A class method's reflection keys on the SINGLETON table, so
        // `Api.method(:fetch)` and `Api.new.method(:fetch)` can't collide --
        // and on the class that WROTE the `def self.x`, not on every
        // descendant materialization gave a copy to, so `Cache.method(:open)`
        // still reports `Store` as its owner.
        let own_cm: Vec<&String> = compiler
            .class(ClassId(id))
            .own_class_methods
            .iter()
            .map(|&sid| {
                let scope = compiler.scope(sid);
                push_method_meta_row(compiler, ClassId(id), scope, true);
                &scope.name
            })
            .collect();
        if !own_cm.is_empty() {
            registrations.push(quote! {
                __registry.mark_own_class_method_rows(zeo_rt::ClassId(#id), &[#(#own_cm),*]);
            });
        }
        // Singleton-chain super targets: every `extend`ed module method's
        // copy -- winner AND shadowed -- registered per `(module, name)` so
        // `call_singleton_super_target` finds the RECEIVER's own copy at
        // any chain position (see `ClassInfo::singleton_super_targets`).
        // Winners reuse the class container's fn; shadowed copies get their
        // own per-class container (same names from different modules would
        // collide in one namespace).
        let ci = compiler.class(ClassId(id));
        let shadowed: Vec<(ClassId, crate::compiler::ScopeId)> = ci
            .singleton_super_targets
            .iter()
            .filter(|(_, sid)| !ci.class_methods.iter().any(|e| e.def == *sid))
            .copied()
            .collect();
        // One container per (class, module): the same NAME can be shadowed
        // in several sibling modules, and each copy is a distinct fn.
        // Deduped: one DEFINITION now serves every position that inherited it,
        // so the same `(module, def)` pair can be reached twice down one chain
        // (a class and its parent both `extend M`). Two copies would emit the
        // same `fn` name twice into one container.
        let mut by_module: Vec<(ClassId, Vec<crate::compiler::ScopeId>)> = Vec::new();
        for &(m, sid) in &shadowed {
            match by_module.iter_mut().find(|(bm, _)| *bm == m) {
                Some((_, sids)) if sids.contains(&sid) => {}
                Some((_, sids)) => sids.push(sid),
                None => by_module.push((m, vec![sid])),
            }
        }
        for (m, sids) in by_module {
            let flat = ident::ident_fragment(&ci.name);
            let container = format_ident!("__sst_{}_{}_{}", idx, m.0, flat);
            // Emitted in THIS class's context, not the module's: the copy's
            // `super` has to resume the singleton chain here, and its class
            // ivars are this class's slots.
            let fns = sids
                .iter()
                .map(|&sid| emit_class_method_fn(compiler, ClassId(id), sid));
            sst_containers.push(quote! {
                #[allow(non_snake_case)]
                pub mod #container {
                    #[allow(unused_imports)]
                    use super::*;
                    #(#fns)*
                }
            });
            for &sid in &sids {
                let scope = compiler.scope(sid);
                let method_ident = ident::class_method_ident(&scope.name);
                let fn_path = quote! { #container::#method_ident };
                let tramp = params::emit_value_trampoline(
                    &fn_path,
                    &scope.name,
                    &scope.params,
                    scope.needs_block_param(),
                    params::RecvMode::Drop,
                    &scope_frame_guard(compiler, scope, true),
                );
                let key = &scope.name;
                let mid = m.0;
                registrations.push(quote! {
                    __registry.define_singleton_super_target(
                        zeo_rt::ClassId(#id),
                        zeo_rt::ClassId(#mid),
                        zeo_rt::Symbol::intern(#key),
                        #tramp,
                    );
                });
            }
        }
        for &(m, sid) in ci
            .singleton_super_targets
            .iter()
            .filter(|(_, sid)| ci.class_methods.iter().any(|e| e.def == *sid))
        {
            let scope = compiler.scope(sid);
            let container = ident::class_ident(compiler, ClassId(id));
            let method_ident = ident::class_method_ident(&scope.name);
            let fn_path = quote! { #container::#method_ident };
            let tramp = params::emit_value_trampoline(
                &fn_path,
                &scope.name,
                &scope.params,
                scope.needs_block_param(),
                params::RecvMode::Drop,
                &scope_frame_guard(compiler, scope, true),
            );
            let key = &scope.name;
            let mid = m.0;
            registrations.push(quote! {
                __registry.define_singleton_super_target(
                    zeo_rt::ClassId(#id),
                    zeo_rt::ClassId(#mid),
                    zeo_rt::Symbol::intern(#key),
                    #tramp,
                );
            });
        }
        user_class_bodies.extend(hoisted_sites_for(ClassId(idx as u32)));
    }

    // Native-exception reopen/subclass DELTAS: the native default method
    // set is installed on every exception id -- the built-in tree by
    // `with_core()`, each user `class MyErr < StandardError` by the
    // `register_exception_subclass` call emitted above. Codegen then emits only
    // the user methods (`!native_default`): a REOPEN of a built-in exception, or
    // a user subclass's own `def`s. Each delta `define_method`s over the native
    // entry AFTER registration, so an override replaces it and an addition
    // extends it -- for the bootstrap classes AND the unified user subclasses
    // alike (`is_exception_backed` spans both).
    let mut exc_containers: Vec<TokenStream> = Vec::new();
    for (idx, _) in compiler.classes.iter().enumerate() {
        if !compiler.is_native_backed(ClassId(idx as u32)) {
            continue;
        }
        if let Some((container, regs)) = emit_exception_deltas(compiler, ClassId(idx as u32)) {
            exc_containers.push(container);
            registrations.extend(regs);
        }
    }

    // Own-`super`-target rows (see `ClassEntry::own_impls`): a `super` walk
    // needs each MRO position's OWN contribution, and generated structs are
    // distinct Rust types -- an ancestor's registered trampoline downcasts
    // to ITS concrete struct and cannot take a subclass receiver. So every
    // own method whose NAME is super-reachable (some same-named method
    // somewhere contains a `super`, `scan_contains_super`) gets a
    // DYNAMIC-SELF bridge fn -- the user-module-bridge shape, correct for
    // any receiver -- registered as that class's own `super` target. The
    // narrowing keeps the second compilation of a body limited to the
    // handful of names `super` can actually reach. NATIVE-backed classes
    // (exception/value subclasses) instead PROMOTE their delta rows: those
    // trampolines downcast to the SHARED native payload type
    // (`RubyException`/`ValueSubclass`), which every subclass instance is.
    // Modules need nothing here -- their own methods are already registered
    // as value methods on their own id (`emit_user_module_bridges`), which
    // the `super` walk probes per position.
    // Precise (class, name) pairs, not names alone: a name-keyed set forced a
    // bridge (a SECOND compilation of the body) onto every same-named own
    // method program-wide -- one `initialize` containing `super` doubled every
    // class's `initialize`. A `super` in a scope defined on X can only ever
    // walk the MRO of a class that has X among its ancestors, and every
    // position it probes is in that class's own `ancestors` list -- so marking
    // (D, name) for each D in ancestors(R), for every class R whose ancestors
    // include X, covers exactly the reachable positions (module-defined
    // `super`s included: R ranges over includers, whose chains continue past
    // the module into their own). The reflection/runtime-def shapes below
    // stay name-global -- their receiver is a runtime choice.
    let mut super_pairs: std::collections::HashSet<(ClassId, &str)> =
        std::collections::HashSet::new();
    let mut super_global: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for scope in &compiler.scopes {
        if !scope
            .body
            .iter()
            .any(|&n| crate::analyze::scan_contains_super(&compiler.hir, n))
        {
            continue;
        }
        let defined_on = scope.defining_class;
        // A `super` in a REFINED method resumes in the refined class's own
        // chain -- CRuby puts the refinement just ahead of the target, so
        // `super` reaches what the refinement overrode. The holder sits on
        // no receiver's ancestry, so the includer scan below finds nothing
        // for it.
        if let Some(target) = compiler.refinement_target(defined_on) {
            for &d in &compiler.class(target).ancestors {
                super_pairs.insert((d, scope.name.as_str()));
            }
        }
        for class in &compiler.classes {
            if class.is_module || !class.ancestors.contains(&defined_on) {
                continue;
            }
            for &d in &class.ancestors {
                super_pairs.insert((d, scope.name.as_str()));
            }
        }
    }
    // `Method#super_method` re-seats a bound Method onto the ancestor that
    // defines the name NEXT, and calling that Method must run the ancestor's
    // body -- which needs exactly the receiver-generic bridge `super` needs.
    // Which name and which ancestor it lands on is a runtime choice, so a
    // program that reflects this way marks every method name reachable. Same
    // over-approximation policy as the runtime-`super` shapes below: a
    // spurious bridge is dead code, a missing one is a wrong NoMethodError.
    if compiler.hir.all_nodes().iter().any(
        |node| matches!(node, crate::hir::HirNode::Call { name, .. } if name == "super_method"),
    ) {
        super_global.extend(compiler.scopes.iter().map(|scope| scope.name.as_str()));
    }
    // RUNTIME-defined methods with a `super` in their body reach targets by
    // NAME through the method-frame walk, so their names count as
    // super-reachable too. Two shapes, both scanned over the whole arena
    // (attribution is local -- the name is an argument of the same node):
    // a `def` in expression position (`Class.new { def g; super; end }`,
    // `HirNode::DefMethod`), and the `class << obj` / `def obj.m` desugar
    // (a `define_singleton_method` call carrying a `SymbolLit` name and a
    // method-body Lambda). The generic Call arm over-approximates
    // deliberately (ANY call whose method-body-lambda argument or literal
    // block contains a `super` marks every SymbolLit argument), covering
    // `send(:define_method, :m) { super }` and friends -- a spurious bridge
    // is dead code, a missing one is a wrong NoMethodError.
    // A `def` some compile-time Scope claims is already covered (precisely)
    // by the pair scan above; only an UNCLAIMED `DefMethod` node is a
    // runtime-defined body whose receiver class is minted at runtime.
    let claimed_defs: std::collections::HashSet<crate::hir::NodeId> =
        compiler.scopes.iter().filter_map(|s| s.def_node).collect();
    for id in compiler.hir.node_ids() {
        match &compiler.hir[id] {
            crate::hir::HirNode::DefMethod { name, body, .. }
                if !claimed_defs.contains(&id)
                    && crate::analyze::scan_contains_super_body(&compiler.hir, body) =>
            {
                super_global.insert(name.as_str());
            }
            crate::hir::HirNode::Call { args, block, .. } => {
                let lambda_super = args.iter().any(|e| {
                    let (crate::hir::ArrayElem::Single(a) | crate::hir::ArrayElem::Splat(a)) = e;
                    matches!(
                        &compiler.hir[*a],
                        crate::hir::HirNode::Lambda { body, method_body: true, .. }
                            if crate::analyze::scan_contains_super_body(&compiler.hir, body)
                    )
                });
                let block_super = block.is_some_and(|b| {
                    matches!(
                        &compiler.hir[b],
                        crate::hir::HirNode::Block { body, .. }
                            if crate::analyze::scan_contains_super_body(&compiler.hir, body)
                    )
                });
                if lambda_super || block_super {
                    for e in args {
                        let (crate::hir::ArrayElem::Single(a) | crate::hir::ArrayElem::Splat(a)) =
                            e;
                        if let crate::hir::HirNode::SymbolLit(n) = &compiler.hir[*a] {
                            super_global.insert(n.as_str());
                        }
                    }
                }
            }
            // A definition hook's pending names take the SAME walk `super`
            // does: the class's own copy is not installed yet while the hook
            // runs, so a call resolves through an ancestor -- and an
            // ancestor's registered trampoline downcasts to ITS struct, so it
            // needs the receiver-generic bridge. Precise, like the pair scan:
            // only the names one hook was told are still ahead, and only over
            // that class's own chain.
            crate::hir::HirNode::DefHook { class, pending, .. } => {
                let ancestors = &compiler.class(ClassId(*class)).ancestors;
                for p in pending {
                    for &d in ancestors {
                        super_pairs.insert((d, p.as_str()));
                    }
                }
            }
            _ => {}
        }
    }
    let mut own_bridge_containers: Vec<TokenStream> = Vec::new();
    for (idx, class) in compiler.classes.iter().enumerate() {
        if idx == 0 || class.is_builtin || class.is_bootstrap || class.is_module {
            continue;
        }
        let cid = ClassId(idx as u32);
        let id = idx as u32;
        let mut bridged: Vec<crate::compiler::ScopeId> = Vec::new();
        for &sid in &class.own_methods {
            let name = &compiler.scope(sid).name;
            if !super_global.contains(name.as_str()) && !super_pairs.contains(&(cid, name.as_str()))
            {
                continue;
            }
            if compiler.is_native_backed(cid) {
                registrations.push(quote! {
                    __registry.promote_own_impl(
                        zeo_rt::ClassId(#id),
                        zeo_rt::Symbol::intern(#name),
                    );
                });
            } else {
                bridged.push(sid);
            }
        }
        if bridged.is_empty() {
            continue;
        }
        let flat = ident::ident_fragment(&class.name);
        let container = format_ident!("__own_{}_{}", idx, flat);
        let fns = bridged
            .iter()
            .map(|&sid| emit_builtin_method_fn(compiler, cid, sid));
        own_bridge_containers.push(quote! {
            #[allow(non_snake_case)]
            pub mod #container { #[allow(unused_imports)] use super::*; #(#fns)* }
        });
        for &sid in &bridged {
            let scope = compiler.scope(sid);
            let method_ident = safe_ident(&scope.name);
            let fn_path = quote! { #container::#method_ident };
            let tramp = params::emit_value_trampoline(
                &fn_path,
                &scope.name,
                &scope.params,
                scope.needs_block_param(),
                params::RecvMode::Pass,
                &scope_frame_guard(compiler, scope, false),
            );
            let key = &scope.name;
            registrations.push(quote! {
                __registry.define_super_target_value(
                    zeo_rt::ClassId(#id),
                    zeo_rt::Symbol::intern(#key),
                    #tramp,
                );
            });
        }
    }

    // A built-in placeholder has no generated `__register` function to call
    // (see the `classes` filter above) -- it still needs a `ClassRegistry`
    // entry of its own, with the SAME linearized `ancestors` every user
    // class gets (computed by `analyze::mro::materialize` uniformly, no
    // special-casing needed there), so `is_a?`/`respond_to?` against a
    // built-in-typed receiver resolve correctly instead of finding nothing.
    let mut builtin_registrations: Vec<TokenStream> = Vec::new();
    for (idx, class) in compiler
        .classes
        .iter()
        .enumerate()
        // `Object` (index 0, not `is_builtin`) registers through the same
        // mapping: its ancestors are COMPUTED (`[Object, Kernel,
        // BasicObject]`), not a hardcoded `vec![Object]`.
        //
        // A require-gated builtin whose feature never fired
        // (`feature_active` false) is SKIPPED: no `require` means no code can
        // resolve its constant (`resolve_class` gates it too), so its
        // registry entry would be pure dead weight -- omitting it is the
        // "register only enabled features" rule that lets an unused extension
        // fall away from the binary.
        .filter(|&(idx, class)| {
            (class.is_builtin || idx == 0) && compiler.feature_active(ClassId(idx as u32))
        })
    {
        let id = idx as u32;
        // The registered display name is the FULLY-QUALIFIED path, not the
        // bare leaf: a nested builtin (`Enumerator::Lazy`, `Digest::SHA256`)
        // is stored as its leaf under a lexical parent (so constant paths
        // resolve into it), but `.name`/`.inspect` must still print the
        // full path.
        let name = compiler.fq_name(ClassId(id));
        let is_module = class.is_module;
        let ancestor_ids = class.ancestors.iter().map(|a| a.0);
        // A per-box OVERLAY never registers a class entry of
        // its own -- instances keep the ROOT builtin's identity -- but
        // its methods/body statements below still run (registered on
        // the root's entry, keyed by the box).
        let is_overlay = class.builtin_overlay.is_some();
        // A REOPENED builtin: each of its methods (own or
        // module-included, all already materialized) registers as a
        // value method so `send_value` dispatches it FIRST -- see
        // `ValueMethodFn`'s docs for the precedence contract. Its
        // `@@cvar = .../CONST = ...` body statements run here too --
        // slightly earlier than their file position (builtins register
        // ahead of user classes), a documented approximation that only
        // matters if a builtin's class body reads a user class.
        for entry in &class.methods {
            let scope = compiler.scope(entry.def);
            let mod_ident = ident::class_ident(compiler, ClassId(id));
            let method_ident = safe_ident(&scope.name);
            let fn_path = quote! { #mod_ident::#method_ident };
            let tramp = params::emit_value_trampoline(
                &fn_path,
                &scope.name,
                &scope.params,
                scope.needs_block_param(),
                params::RecvMode::Pass,
                &scope_frame_guard(compiler, scope, false),
            );
            let ci = compiler.class(ClassId(id));
            // A per-box OVERLAY's methods register on the ROOT
            // builtin's entry, keyed by the overlay's box. The row KEY is
            // the real Ruby name, not the escaped Rust ident -- same
            // reasoning as `emit_class`'s `dispatch_key`.
            let target = ci.builtin_overlay.map_or(id, |root| root.0);
            push_vm_row(target, ci.box_id, &scope.name, tramp);
            // A PRIVATE `def` (every top-level def, and an explicit
            // `private def x`) is recorded so `respond_to?` skips it; a
            // PROTECTED one so the `protected_*` reflection reports it.
            match scope.visibility {
                crate::hir::Visibility::Private => push_vis_row(target, &scope.name, 0),
                crate::hir::Visibility::Protected => push_vis_row(target, &scope.name, 1),
                crate::hir::Visibility::Public => {}
            }
            // Bake this method's reflection facts -- the `own_methods` loop
            // above only covers user CLASSES, not a reopened builtin (Object,
            // which every top-level `def` materializes onto).
            push_method_meta_row(compiler, ClassId(target), scope, false);
        }
        // A `def self.x` on a REOPENED builtin -- or one written in a `class <<
        // Time` body -- needs the same dynamic-dispatch row a user class gets
        // (see the `class_methods` loop in the user-class block above). Without
        // it the method answers only a call codegen resolved statically, so
        // `Time.rfc2822(s)` worked while the `self.rfc2822(s)` inside time.rb's
        // own `httpdate` did not, and `Time.respond_to?(:rfc2822)` said false.
        let own_cm: Vec<&String> = class
            .class_methods
            .iter()
            .map(|entry| {
                let scope = compiler.scope(entry.def);
                let mod_ident = ident::class_ident(compiler, ClassId(id));
                let method_ident = ident::class_method_ident(&scope.name);
                let fn_path = quote! { #mod_ident::#method_ident };
                let tramp = params::emit_value_trampoline(
                    &fn_path,
                    &scope.name,
                    &scope.params,
                    scope.needs_block_param(),
                    params::RecvMode::Drop,
                    &scope_frame_guard(compiler, scope, true),
                );
                push_cm_row(id, &scope.name, tramp);
                push_method_meta_row(compiler, ClassId(id), scope, true);
                &scope.name
            })
            .collect();
        if !own_cm.is_empty() {
            registrations.push(quote! {
                __registry.mark_own_class_method_rows(zeo_rt::ClassId(#id), &[#(#own_cm),*]);
            });
        }
        // The CLASS-method visibility half (verbs 3/4), the same rows the
        // user-class loop emits. Without it a `private` inside `class << self`
        // on a REOPENED BUILTIN was enforced on the call (codegen knows the
        // def is private) but recorded nowhere, so `respond_to?` and
        // `singleton_methods` both reported the method as public.
        for entry in &class.class_methods {
            if entry.visibility == crate::hir::Visibility::Private {
                push_vis_row(id, compiler.names.str(entry.name), 3);
            }
        }
        for (cm_name, vis) in &class.class_visibility_overrides {
            let verb = if *vis == crate::hir::Visibility::Private {
                3
            } else {
                4
            };
            push_vis_row(id, cm_name, verb);
        }
        // A reopened builtin's `private_constant`, same listing fact the
        // user-class loop records.
        let priv_consts: Vec<&str> = class.private_constants.iter().map(String::as_str).collect();
        if !priv_consts.is_empty() {
            registrations.push(quote! {
                zeo_rt::const_set_private(#id, &[#(#priv_consts),*], true);
            });
        }
        // A reopened builtin's own `extend M`, same singleton-chain fact the
        // user-class loop records (`class Array; extend M; end`).
        let extend_ids: Vec<u32> = class.extends.iter().map(|m| m.0).collect();
        if !extend_ids.is_empty() {
            registrations.push(quote! {
                __registry.register_extends(
                    zeo_rt::ClassId(#id),
                    vec![#(zeo_rt::ClassId(#extend_ids)),*],
                );
            });
        }
        builtin_class_bodies.extend(hoisted_sites_for(ClassId(id)));
        // Always-on builtins with their DEFAULT ancestors are registered
        // once by `zeo_rt::register_builtins` -- so emit a base register
        // here only for a require-gated extension (per-program, and the loop
        // already feature-gates it) or a builtin whose ancestors a reopen
        // actually changed (`class Array; include M; end`). The latter is an
        // OVERRIDE: it lands after `register_builtins` in `main` and replaces
        // the default entry. This keeps the common program free of the ~540
        // identical builtin registrations while preserving full reopen parity.
        let is_ext = zeo_abi::is_gated_builtin(ClassId(id));
        let ancestors_default = class.ancestors == zeo_abi::declared_ancestors(ClassId(id));
        let register = (!is_overlay && (is_ext || !ancestors_default)).then(|| {
            quote! {
                __registry.register(
                    zeo_rt::ClassId(#id),
                    #name,
                    #is_module,
                    vec![#(zeo_rt::ClassId(#ancestor_ids)),*],
                    None,
                );
            }
        });
        // Aliases of builtin methods recorded on a REOPENED builtin (or on
        // `Object` itself, where a top-level `alias_method` lands) -- same
        // rows the user-class loop emits; an overlay's rows land on the
        // root's entry like its methods do.
        let alias_target_id = class.builtin_overlay.map_or(id, |root| root.0);
        let alias_rows = class.builtin_aliases.iter().map(|(new, old)| {
            quote! {
                __registry.register_alias(zeo_rt::ClassId(#alias_target_id), #new, #old);
            }
        });
        let class_alias_rows = class.class_aliases.iter().map(|(new, old)| {
            quote! {
                __registry.register_class_alias(zeo_rt::ClassId(#alias_target_id), #new, #old);
            }
        });
        builtin_registrations.push(quote! {
            #register
            #(#alias_rows)*
            #(#class_alias_rows)*
        });
    }

    // Builtin-alias rows exist -> validate them (`NameError` for a source that
    // resolves nowhere). A class with a BODY SITE validates there instead, at
    // the position its `alias_method` was written -- CRuby's own timing, and
    // the only one that sees a source the body itself defines. What is left
    // here is the classes with no body of their own to run. Omitted entirely
    // for the common aliasless program, whose generated source is unchanged.
    let unbodied: Vec<u32> = compiler
        .classes
        .iter()
        .enumerate()
        .filter(|(i, c)| {
            !c.builtin_aliases.is_empty()
                && !compiler
                    .class_body_sites
                    .iter()
                    .any(|s| s.class.0 as usize == *i)
        })
        .map(|(i, _)| i as u32)
        .collect();
    let validate_aliases = (!unbodied.is_empty()).then(|| {
        quote! { #(zeo_rt::validate_class_aliases(zeo_rt::ClassId(#unbodied))?;)* }
    });

    let main_label_counter = Cell::new(0u32);
    // Top-level implicit-self calls dispatch on the global `main_object()`
    // (no capture needed), so no `self_class` here.
    // `TOPLEVEL_BINDING` IS the top-level frame's binding, so a program that
    // can read it -- anywhere, including inside a required gem's method
    // (erb's `new_toplevel`) -- deoptimizes the top level to cells exactly as
    // a literal `binding` call there would.
    let wants_toplevel_binding = compiler
        .hir
        .all_nodes()
        .iter()
        .any(|n| matches!(n, crate::hir::HirNode::ClassRef(c) if c == "TOPLEVEL_BINDING"));
    let mut main_captures = captures::collect_escaping_captures(
        compiler,
        &analyzed.main_statements,
        &crate::hir::Params::default(),
        class_query::SelfClass::default(),
    );
    let main_binding = captures::binding_scope_names(
        compiler,
        &analyzed.main_statements,
        &crate::hir::Params::default(),
        &mut main_captures,
        wants_toplevel_binding,
    );
    let cx = Ctx {
        compiler,
        box_id: 0,
        current_class: None,
        defining_class: None,
        // Top-level `self` is `main`, an ordinary Object -- not a class.
        class_self: None,
        current_method: None,
        current_method_origin: None,
        local_types: binding_scope_local_types(
            main_binding.as_ref(),
            &main_captures.locals,
            &analyzed.main_local_types,
        ),
        label_counter: &main_label_counter,
        loop_labels: None,
        next_yields_value: false,
        for_var_override: None,
        captured_locals: std::borrow::Cow::Borrowed(&main_captures.locals),
        binding_names: main_binding.clone(),
        in_eval_splice: false,
        self_ident: format_ident!("self"),
        in_real_proc: false,
        self_is_dynamic: false,
        self_slots: false,
        shared_body: false,
        trace: None,
        runtime_super_params: None,
        defined_by_define_method: false,
        lexical_frame_label: None,
        block_depth: 0,
        has_blk_binding: false,
    };
    // Installed UNCONDITIONALLY: `Object.constants` must list it (census).
    // A program that never names it gets the cheap degraded form -- self =
    // `main`, no locals -- because `binding_names` stayed `None` and the top
    // level never deoptimized to cells; naming it anywhere upgrades both.
    let toplevel_binding = {
        let value = call::emit_binding_value(&cx, "<main>", 0);
        quote! { zeo_rt::const_set(0, "TOPLEVEL_BINDING", #value); }
    };
    let main_body = hoisting::emit_hoisted_body_after_decls(
        &cx,
        &analyzed.main_statements,
        quote! { #toplevel_binding },
        true,
    );
    // The compiled-in load path: one function per unit, plus the table the
    // runtime resolves a computed `require`/`autoload` against. Each body is
    // hoisted exactly like the main body -- a unit IS a top-level scope, with
    // its own file-isolated locals. See `zeo_rt::features`.
    let mut unit_fns = Vec::new();
    let mut unit_rows = Vec::new();
    for (i, (feature, absolute, stmts)) in analyzed.feature_units.iter().enumerate() {
        let ident = proc_macro2::Ident::new(&format!("__unit_{i}"), proc_macro2::Span::call_site());
        // A unit is its own top-level scope, so it needs its own captures and
        // binding names -- derived from ITS statements, exactly as `main`'s are
        // derived above.
        //
        // Emitting a unit under MAIN's `cx` made every `binding` built inside
        // one list main's captured cells, which a free `fn __unit_N` cannot
        // see. activemodel is the case: a top-level `rescue LoadError => e`
        // spliced in from `xml_mini/nokogiri.rb` becomes one `__f9_e` cell in
        // main, and seven units emitted `("__f9_e", Arc::clone(&__f9_e))` for
        // it -- `cannot find value __f9_e in this scope`, once per unit.
        //
        // `wants_toplevel_binding` is deliberately NOT passed on: that flag
        // deoptimizes the frame `TOPLEVEL_BINDING` names, which is main's, not
        // a unit's. A unit that calls `binding` itself still deoptimizes,
        // because `binding_scope_names` finds that call in its own statements.
        let mut unit_captures = captures::collect_escaping_captures(
            compiler,
            stmts,
            &crate::hir::Params::default(),
            class_query::SelfClass::default(),
        );
        let unit_binding = captures::binding_scope_names(
            compiler,
            stmts,
            &crate::hir::Params::default(),
            &mut unit_captures,
            false,
        );
        let unit_cx = Ctx {
            local_types: binding_scope_local_types(
                unit_binding.as_ref(),
                &unit_captures.locals,
                &analyzed.main_local_types,
            ),
            captured_locals: std::borrow::Cow::Owned(unit_captures.locals.clone()),
            binding_names: unit_binding,
            ..cx.clone()
        };
        let body = hoisting::emit_hoisted_body_after_decls(&unit_cx, stmts, quote! {}, true);
        unit_fns.push(quote! {
            fn #ident() -> Result<zeo_rt::RubyValue, zeo_rt::Signal> {
                #body
            }
        });
        // Both spellings a program can build resolve to the same unit: the
        // load-path-relative feature name, and the absolute path
        // `File.expand_path("x", __dir__)` produces.
        let f = quote! { #ident as fn() -> Result<zeo_rt::RubyValue, zeo_rt::Signal> };
        unit_rows.push(quote! { (#feature, #f) });
        unit_rows.push(quote! { (#absolute, #f) });
    }
    let install_units = if unit_rows.is_empty() {
        quote! {}
    } else {
        quote! {
            zeo_rt::features::install_feature_units(&[#(#unit_rows),*]);
        }
    };
    let mut declined_rows = Vec::new();
    for (feature, absolute, reason) in &analyzed.declined_units {
        declined_rows.push(quote! { (#feature, #reason) });
        declined_rows.push(quote! { (#absolute, #reason) });
    }
    let install_declined = if declined_rows.is_empty() {
        quote! {}
    } else {
        quote! {
            zeo_rt::features::install_declined_features(&[#(#declined_rows),*]);
        }
    };
    // The top level's own backtrace frame -- CRuby's `<main>` (its file is
    // the main script; statement emission stamps the line as it goes).
    let main_frame = match compiler.hir.files.first() {
        Some(f) => {
            let file = pooled_file(&f.name);
            quote! { let __frame = zeo_rt::FrameGuard::push(#file, "<main>", 0, 0); }
        }
        None => quote! {},
    };

    // Every spliced file's canonical path, for `$LOADED_FEATURES`. Build-machine
    // paths, baked in -- the same caveat `$LOAD_PATH` carries (see
    // `docs/COMPATIBILITY.md`); what they are FOR is letting a runtime require of
    // an already-spliced file answer `false` rather than raise.
    let mut loaded_features: Vec<String> = compiler
        .hir
        .loaded_files
        .iter()
        .map(|f| f.canonical.to_string_lossy().into_owned())
        .collect();
    // `ruby` preloads rbconfig (via rubygems) before the first program line,
    // so its `$LOADED_FEATURES` entry exists in EVERY process -- including
    // programs whose demand-driven splice skipped the shim's code. Listing
    // it keeps a dynamic `require "rbconfig"` answering `false` either way.
    let ambient_rbconfig = "<zeo-shim>/rbconfig.rb".to_string();
    if !loaded_features.contains(&ambient_rbconfig) {
        loaded_features.push(ambient_rbconfig);
    }

    // Ruby's own parse warnings, as one literal slice -- emitted only when
    // the program actually has some, so the common binary carries nothing.
    let parse_warnings = (!compiler.hir.warnings.is_empty()).then(|| {
        let lines = compiler.hir.warnings.iter().map(ToString::to_string);
        quote! { zeo_rt::emit_parse_warnings(&[#(#lines),*]); }
    });

    // `DATA` -- emitted only for a script that has an `__END__`, so every other
    // binary neither carries the path nor opens anything at startup.
    let data_section = compiler.hir.data_section.as_ref().map(|d| {
        let (path, offset) = (&d.path, d.offset);
        quote! { zeo_rt::install_data_section(#path, #offset); }
    });

    // User-module method bridges (see `emit_user_module_bridges`): their value-
    // method registrations ride `__VM_ROWS`, applied after every module's own
    // `__registry.register` above has created the entry they attach to.
    let um_containers = emit_user_module_bridges(compiler);
    let redef_containers = emit_redef_containers(compiler);

    // Every body has been emitted by now, so the coverage collector (fed by
    // `stmt::stamp_line` during those emissions) is complete.
    let coverage_install = coverage_install_tokens(compiler);

    // Exceptions are constructed at runtime by NAME from the registered
    // classes (`ClassRegistry::construct_exception`) -- no per-program
    // factory. The prelude classes register their `ConstructorFn` via
    // `ruby_class!`'s `__register`, which is what the runtime construction
    // path uses.

    // The batched registration-row calls. Safe to read the pools here:
    // every `__VM_ROWS`/`__CM_ROWS`/`__VIS_ROWS` row is pushed by the
    // registration loops and bridge emitter ABOVE, not by anything the
    // program quote below interpolates lazily.
    let row_calls = POOLS.with_borrow(|p| {
        let vm =
            (!p.vm_rows.is_empty()).then(|| quote! { __registry.define_value_rows(__VM_ROWS); });
        let cm =
            (!p.cm_rows.is_empty()).then(|| quote! { __registry.define_class_rows(__CM_ROWS); });
        let vis = (!p.vis_rows.is_empty())
            .then(|| quote! { __registry.mark_visibility_rows(__VIS_ROWS); });
        quote! { #vm #cm #vis }
    });

    // Sorted so the generated source is stable across runs (the set is a hash
    // set). Almost always empty -- reopening `Module` to hook every definition
    // in the program is a rare and deliberate thing to write.
    let global_def_hooks: Vec<TokenStream> = {
        let mut names: Vec<&String> = compiler.global_def_hooks.iter().collect();
        names.sort();
        names
            .into_iter()
            .map(|n| quote! { zeo_rt::mark_global_def_hook(#n); })
            .collect()
    };

    // Observable redefinition timelines: the FIRST body goes into the
    // overlay before the first statement runs, so the window before each
    // reopen's re-install (`HirNode::MethodRedefine`, emitted at document
    // position) dispatches the way ruby's install-where-it-stands does.
    let boot_redefs: Vec<TokenStream> = compiler
        .positional_redefs
        .iter()
        .map(|&(cid, ref name, sid)| {
            let tramp = redef_trampoline(compiler, cid, sid);
            let id = cid.0;
            quote! {
                zeo_rt::runtime_replace_method(
                    zeo_rt::ClassId(#id),
                    zeo_rt::Symbol::intern(#name),
                    #tramp,
                );
            }
        })
        .collect();

    // Compile-registered singleton-class surrogates (constant-bearing
    // `class << self` bodies): seed the runtime mint so `M.singleton_class`
    // answers the surrogate that owns the constants.
    let singleton_surrogates: Vec<TokenStream> = compiler
        .classes
        .iter()
        .enumerate()
        .filter(|&(i, _)| compiler.is_singleton_surrogate(ClassId(i as u32)))
        .filter_map(|(i, c)| {
            c.lexical_parent.map(|owner| {
                let (o, s) = (owner.0, i as u32);
                quote! {
                    zeo_rt::register_singleton_surrogate(
                        zeo_rt::ClassId(#o),
                        zeo_rt::ClassId(#s),
                    );
                }
            })
        })
        .collect();

    // Every hoisted class body has been lifted by now.
    let class_body_fns = class_body_fns.into_inner();

    let program = quote! {
        // Lints that mirror RUBY-source properties, not codegen defects: an
        // unused Ruby assignment (or a hoisted local never read), code after
        // a `raise`, Kernel#URI's own capitalization. Genuine-defect lints
        // (unused_must_use and friends) stay live -- the generated program is
        // expected to build clean.
        #![allow(
            unused_parens,
            unused_braces,
            unused_mut,
            unused_variables,
            unreachable_code,
            unused_assignments,
            non_snake_case
        )]

        // Statically-linked binaries (`zeo foo.rb -o app`, the bench suite)
        // swap in mimalloc: Ruby workloads are allocation-heavy, and a
        // self-contained binary owns every allocation. The backend passes
        // the cfg only for `Linkage::Static` -- a dynamic-linked test binary
        // must never free the shared dylib's allocations with a different
        // allocator.
        #[cfg(zeo_static_alloc)]
        #[global_allocator]
        static __ALLOC: zeo_rt::MiMalloc = zeo_rt::MiMalloc;

        #shared_container
        #(#classes)*
        #(#class_method_containers)*
        #(#builtin_reopens)*
        #(#um_containers)*
        #(#redef_containers)*
        #(#exc_containers)*
        #(#own_bridge_containers)*
        #(#sst_containers)*
        #(#unit_fns)*
        #(#class_body_fns)*

        fn main() {
            // This thread is the only Ruby thread until the program spawns
            // one, which is what lets an instance-variable access reach its
            // slot without locking (`zeo_rt::IvarCell`). Every spawn site in
            // the runtime clears it first, and no other thread ever sets it.
            zeo_rt::mark_sole_thread();
            // A registry pre-populated with the CORE world -- the always-on
            // builtin classes/modules AND the built-in exception hierarchy,
            // installed once from `zeo-rt`. Their ids are the ones `zeo-abi`
            // reserves and the compiler asserts it assigned identically (see
            // `analyze`).
            let mut __registry = zeo_rt::ClassRegistry::with_core();
            // Require-gated exts and reopen-modified builtins register on top
            // (the latter as an override that replaces the default entry),
            // then the program's own user classes.
            #(#builtin_registrations)*
            #(#registrations)*
            // The batched registration rows (`__VM_ROWS`/`__CM_ROWS`/
            // `__VIS_ROWS` -- see `PoolBuilder`), applied after every
            // `register` above so each row's entry exists.
            #row_calls
            zeo_rt::install_class_registry(__registry);
            // Every `def`'s reflection facts, from the one static table at
            // the bottom of this file (see `push_method_meta_row`).
            zeo_rt::register_meta_rows(__META_ROWS);
            // Definition hooks the program put on `Module`/`Class`/
            // `BasicObject` themselves, which answer for every class. The
            // runtime's per-class owner scan cannot infer these -- see
            // `Compiler::global_def_hooks`.
            #(#global_def_hooks)*
            // First bodies of observably-redefined methods -- see
            // `Compiler::positional_redefs`.
            #(#boot_redefs)*
            #(#singleton_surrogates)*
            // Seed the CORE constants (`Float::INFINITY`, `Encoding::UTF_8`,
            // `Regexp::IGNORECASE`, `ARGV`, `STDOUT`/`$stdout`, `ENV`,
            // `Process::CLOCK_*`) -- their owners resolved at compile time;
            // only the values need installing.
            zeo_rt::install_core_constants();
            // Every file the front end spliced, as `$LOADED_FEATURES` -- so a
            // dynamic `require`/`require_relative` of one (net/smtp globs its
            // authenticators and requires each) answers `false`, the way Ruby
            // answers an already-loaded feature, instead of raising LoadError
            // because an AOT binary has no runtime loader.
            zeo_rt::seed_loaded_features(&[#(#loaded_features),*]);
            // Before the first statement: a `require` on line one must
            // already see the compiled-in load path.
            #install_units
            #install_declined
            #coverage_install
            // Ruby's own PARSE-time warnings (a duplicated hash key, ...),
            // collected by the front end and replayed before the program's
            // first line -- where CRuby prints them.
            #parse_warnings
            #data_section
            // The runtime raises real, catchable exceptions (NoMethodError,
            // ArgumentError, TypeError, StopIteration, ...) by constructing
            // them itself from the registered classes -- see
            // `ClassRegistry::construct_exception`. No per-program factory is
            // installed.

            // The top-level program body runs on the main OS thread -- see
            // `zeo_rt::run_main`'s docs for the worker-count / GVL model and
            // why registration must complete first. The closure is
            // `move + Send + 'static` trivially: top-level statements are
            // self-contained (their hoisted locals are declared inside the
            // body itself) and every value is Send+Sync.
            let __result: Result<zeo_rt::RubyValue, zeo_rt::Signal> =
                zeo_rt::run_main(move || {
                    // Class-body statements (`@@x = expr` / `CONST = expr`)
                    // run first, inside the fallible closure (they may
                    // `?`), builtins before user classes, before every
                    // top-level statement.
                    #main_frame
                    zeo_rt::check_ints()?;
                    #validate_aliases
                    #(#builtin_class_bodies)*
                    #(#user_class_bodies)*
                    #main_body
                });
            // `at_exit` handlers (reverse order), before uncaught-exception
            // reporting -- CRuby runs them on both the normal and the
            // uncaught path. (`Kernel#exit` runs them itself.)
            zeo_rt::run_at_exit();
            // Then the `ObjectSpace.define_finalizer` sweep: CRuby finalizes
            // every remaining object at exit, after `at_exit`.
            zeo_rt::run_finalizers();
            if let Err(__signal) = __result {
                match __signal {
                    // An uncaught `raise` renders CRuby's full report
                    // (`file:line:in 'frame': msg (Class)` + tab-indented
                    // `from` lines) from the backtrace stamped at raise
                    // time -- see `zeo_rt::report_uncaught`.
                    //
                    // An uncaught `SystemExit` is not an error at all: it is
                    // how `exit`/`abort` terminate, so the process just exits
                    // with the carried status, silently (`at_exit` has already
                    // run above).
                    zeo_rt::Signal::Raise(__exc) => {
                        if let Some(__code) = zeo_rt::system_exit_status(&__exc) {
                            std::process::exit(__code);
                        }
                        zeo_rt::report_uncaught(&__exc);
                        std::process::exit(1);
                    }
                    // `return` written at the top level ENDS the program,
                    // silently and successfully -- CRuby's rule. It reaches
                    // here only from a position that raises the signal rather
                    // than returning literally (inside a `rescue`/`ensure`
                    // clause, say); the plain statement form folds earlier.
                    // A `return` at the top level of a REQUIRED file ends the
                    // whole program rather than just that file's load, since
                    // zeo splices the file into its requirer -- see
                    // `docs/COMPATIBILITY.md`.
                    zeo_rt::Signal::Return(_) => {}
                    __other => {
                        eprintln!("uncaught signal escaped the top level: {:?}", __other);
                        std::process::exit(1);
                    }
                }
            }
        }
    };
    // The literal pools, appended AFTER the program tokens are fully built --
    // interpolation above is what runs the lazy emitters that register pool
    // entries, so the tables must be read only now. See `PoolBuilder`.
    let pools = take_pools();
    let syms = (!pools.syms.is_empty()).then(|| {
        let names = &pools.syms;
        quote! { static __SYMS: zeo_rt::SymPool = zeo_rt::SymPool::new(&[#(#names),*]); }
    });
    let lits = (!pools.lits.is_empty()).then(|| {
        let texts = &pools.lits;
        quote! { static __LITS: zeo_rt::LitPool = zeo_rt::LitPool::new(&[#(#texts),*]); }
    });
    // A plain `[&str; n]`, not a pool type: a frame guard wants the `&'static
    // str` itself, so there is nothing to intern or memoize on the way out.
    let files = (!pools.files.is_empty()).then(|| {
        let paths = &pools.files;
        let n = pools.files.len();
        quote! { static __FILES: [&str; #n] = [#(#paths),*]; }
    });
    // ONE array, not one static per site: a `static` item each cost rustc
    // 155% on uri and 26% more emitted lines, for storage that is identical
    // either way.
    // ...and one array per CALLER class, so the visibility question a site
    // asks rides in the site itself rather than in an argument every dynamic
    // call would have to pass. A program has few classes, so this stays a
    // handful of arrays.
    let call_sites = pools.call_sites.iter().map(|(&caller, &n)| {
        let arr = call_site_array(caller);
        quote! {
            static #arr: [zeo_rt::CallSite; #n] =
                [const { zeo_rt::CallSite::new(#caller) }; #n];
        }
    });
    let pps = &pools.pps;
    let metas = &pools.metas;
    let vm_rows = (!pools.vm_rows.is_empty()).then(|| {
        let rows = &pools.vm_rows;
        quote! {
            static __VM_ROWS: &[(u32, u32, &str, zeo_rt::ValueMethodFn)] = &[#(#rows),*];
        }
    });
    let cm_rows = (!pools.cm_rows.is_empty()).then(|| {
        let rows = &pools.cm_rows;
        quote! {
            static __CM_ROWS: &[(u32, &str, zeo_rt::ValueMethodFn)] = &[#(#rows),*];
        }
    });
    let vis_rows = (!pools.vis_rows.is_empty()).then(|| {
        let rows = &pools.vis_rows;
        quote! {
            static __VIS_ROWS: &[(u32, &str, u8)] = &[#(#rows),*];
        }
    });
    quote! {
        #program #syms #lits #files #(#call_sites)* #(#pps)*
        static __META_ROWS: &[zeo_rt::MetaRow] = &[#(#metas),*];
        #vm_rows #cm_rows #vis_rows
    }
}

/// Where a class body's value comes from when its last SOURCE statement is one
/// analyze consumed, so the emitted statements no longer end where ruby's value
/// does.
enum TailValue {
    /// The emitted body's own tail is already the answer.
    Own,
    /// A consumed construct whose ruby value is known.
    Known(TokenStream),
    /// A consumed construct with no value form yet -- carries the node to blame
    /// and its ruby spelling.
    Unknown(crate::hir::NodeId, &'static str),
}

/// [`TailValue`] for one site.
///
/// A `def` is the case that matters and the only one wired: ruby answers the
/// method's name, and `class C; def a; end; end` has no surviving statement at
/// all. Every other consumed construct has its own value (`include M` answers
/// the class, `attr_accessor :x` the accessor names) and comes back `Unknown`.
fn consumed_tail_value(compiler: &Compiler, site: &crate::compiler::ClassBodySite) -> TailValue {
    let Some(def_node) = site.def_node else {
        return TailValue::Own;
    };
    let crate::hir::HirNode::ClassDef { body, .. } = &compiler.hir[def_node] else {
        return TailValue::Own;
    };
    let Some(&last) = body.last() else {
        return TailValue::Own;
    };
    if site.stmts.last() == Some(&last) {
        return TailValue::Own;
    }
    match &compiler.hir[last] {
        crate::hir::HirNode::DefMethod { name, .. } => {
            let sym = pooled_sym(name);
            TailValue::Known(quote! { zeo_rt::RubyValue::Symbol(#sym) })
        }
        node => TailValue::Unknown(last, crate::codegen::expr::definition_kind(node)),
    }
}

/// Whether the emitted site EVALUATES to the class body's last value.
///
/// A `class`/`module` is an expression in Ruby -- `x = class C; 7; end` binds
/// 7, and an empty body yields nil. Almost every site runs for effect, so the
/// value is dropped; only a definition written where a value is read keeps it.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum BodyValue {
    Discard,
    /// A statement in TAIL position: the value is the enclosing scope's, so it
    /// is kept where it can be computed. A body ending in a construct analyze
    /// consumed and [`consumed_tail_value`] cannot name falls back to nil --
    /// the long-standing narrow divergence, now down to those constructs
    /// alone. Not an error, because `class Foo; alias bar baz; end` as a
    /// file's last statement is ordinary and its value is never read.
    KeepOrNil,
    /// An EXPRESSION: something definitely reads this value, so a body whose
    /// value zeo cannot compute is a compile error rather than a wrong answer.
    Keep,
}

/// A `HirNode::ClassDef` marker, emitted at the position it was written --
/// the one lookup both the statement form (`stmt::emit_statement`) and the
/// value form (`expr::emit_expr`) share.
///
/// Every statement written directly in a class/module body -- cvar/const/ivar
/// writes AND general code (a method call, an `each` loop, a runtime
/// `define_method`) -- runs once at class-definition time, in file order, with
/// `self` = the class object.
///
/// A marker with NO registered site is a `class`/`module` the analyze walk
/// never reached (inside a top-level `begin`, or an undecided body-level
/// `if`). That is a separate, catalogued gap: it stays loud rather than
/// silently skipping the definition.
fn emit_class_def_marker(cx: &Ctx, stmt: crate::hir::NodeId, value: BodyValue) -> TokenStream {
    let site = cx
        .compiler
        .class_body_sites
        .iter()
        .find(|s| s.def_node == Some(stmt));
    let Some(site) = site else {
        // Name the definition: this fires deep in a require graph, and the
        // identity is what makes the next blocker legible. The LOCATION rides
        // in the diagnostic's span rather than the text, so the message stays
        // free of a local path.
        let nm = match &cx.compiler.hir[stmt] {
            crate::hir::HirNode::ClassDef { name, .. } => name.as_str(),
            _ => "?",
        };
        return unsupported_at(
            cx.compiler,
            stmt,
            format!(
                "`class`/`module` in a position the analyze walk doesn't register \
                 isn't supported yet (zeo limitation): {nm}"
            ),
        );
    };
    let (item, call) =
        emit_class_body_site_lifted(cx.compiler, site, &cx.captured_locals, None, value);
    debug_assert!(item.is_empty(), "no lift was asked for");
    call
}

/// The same body, as its OWN compiled unit: `(fn item, call site)`.
///
/// A class body is a separate scope in Ruby, not a region of the enclosing
/// one. CRuby says so structurally -- `NODE_CLASS` compiles to
/// `NEW_CHILD_ISEQ(..., ISEQ_TYPE_CLASS)` (compile.c), a child iseq with its
/// own local table, which is why `x = 1; class Foo; p defined?(x); end` prints
/// nil. Only the superclass expression is compiled into the *enclosing* iseq,
/// where it can read `x`.
///
/// zeo already obeys the scope rule -- the body hoists its own locals and the
/// `{ }` below keeps them from leaking -- so the block is closed over nothing
/// and moving it into a free `fn` is the same program. What that buys is a
/// `fn main` that is not the whole class-definition phase of the program in
/// one function body: at Rails scale the hoisted bodies are megabytes of it,
/// and rustc's per-function work grows faster than linearly.
///
/// The lift is checked by rustc, not by us: a free `fn` captures nothing, so
/// a body that did reach for an enclosing local fails to compile instead of
/// quietly reading the wrong one.
pub(crate) fn emit_class_body_site_lifted(
    compiler: &Compiler,
    site: &crate::compiler::ClassBodySite,
    enclosing_captured: &HashSet<String>,
    lift: Option<u32>,
    value: BodyValue,
) -> (TokenStream, TokenStream) {
    let cid = site.class;
    // `alias_method`'s source is checked as THIS body runs -- CRuby's timing,
    // and the only one that sees a source the body itself defines (bundler's
    // `Runtime` does `definition_method :specs`, a `define_method` wrapper,
    // and then `alias_method :gems, :specs`). Emitted even for an otherwise
    // empty body, which is what an alias-only class has.
    let alias_check = (!compiler.class(cid).builtin_aliases.is_empty()).then(|| {
        let id = cid.0;
        quote! { zeo_rt::validate_class_aliases(zeo_rt::ClassId(#id))?; }
    });
    // `class Foo; end` DEFINES a constant, so ruby announces it -- before
    // `inherited` and before the body, which is the order `vm_declare_class`
    // hard-codes (`declare_under` sets the constant, then `rb_class_inherited`,
    // then the body runs).
    let const_added = emit_declaration_const_added(compiler, site);
    let const_location = emit_declaration_const_location(compiler, site);
    let inherited_hook = emit_inherited_hook(compiler, site);
    let stmts = &site.stmts;
    // A body whose last SOURCE statement analyze consumed -- the emitted
    // statements no longer end where ruby's value comes from. Computed before
    // the empty-body return, which is exactly the `class C; def a; end; end`
    // case: no statement survives, and the value is still `:a`.
    //
    // `own` is what the block itself evaluates to, which differs between the
    // two returns below (nothing ran, versus the body's own tail).
    let tail_value = |own: TokenStream| match (value, consumed_tail_value(compiler, site)) {
        (BodyValue::Discard, _) | (_, TailValue::Own) => own,
        (_, TailValue::Known(v)) => v,
        (BodyValue::KeepOrNil, TailValue::Unknown(..)) => quote! { zeo_rt::RubyValue::Nil },
        (BodyValue::Keep, TailValue::Unknown(at, kind)) => unsupported_at(
            compiler,
            at,
            format!("a class body read for its VALUE cannot end in {kind} yet (zeo limitation)"),
        ),
    };
    if stmts.is_empty() {
        let head = quote! { #alias_check #const_location #const_added #inherited_hook };
        return match value {
            BodyValue::Discard => (quote! {}, head),
            _ => {
                let tail = tail_value(quote! { zeo_rt::RubyValue::Nil });
                (quote! {}, quote! { { #head #tail } })
            }
        };
    }
    let label_counter = Cell::new(0u32);
    // A class body is an ordinary Ruby scope with ordinary locals, and an
    // escaping block written in it closes over them exactly as one written at
    // the top level does (`yesno = CompletingHash.new; %w[- no].each { |el|
    // yesno[el] = false }`, optparse's own accept-table setup). Left empty,
    // every such name was re-declared nil INSIDE the closure.
    let mut captures = captures::collect_escaping_captures(
        compiler,
        stmts,
        &crate::hir::Params::default(),
        class_query::SelfClass::default(),
    );
    let binding_names = captures::binding_scope_names(
        compiler,
        stmts,
        &crate::hir::Params::default(),
        &mut captures,
        false,
    );
    // A site emitted INLINE at its document position sits inside the
    // enclosing scope's own Rust block, so a name that scope keeps in a cell
    // is a cell here too. A guarded reopen is the shape that reaches for one
    // -- pp.rb's `class Set ... end if set_pp` carries the top-level `set_pp`
    // in as its guard. The class body's OWN locals still win: a class body is
    // a fresh Ruby scope, and its names shadow rather than share.
    if !enclosing_captured.is_empty() {
        let mut own = Vec::new();
        for &n in stmts {
            hoisting::collect_locals(compiler, n, &mut own);
        }
        captures.locals.extend(
            enclosing_captured
                .iter()
                .filter(|n| !own.contains(n))
                .cloned(),
        );
    }
    let no_locals = HashMap::new();
    let cx = Ctx {
        compiler,
        box_id: compiler.class(cid).box_id,
        // NOT `Some(cid)`: `current_class` means "there is a concrete
        // `self: Arc<Self>` receiver in scope", and a class body has none
        // -- it runs from `main()`. `self` in a class body is the CLASS
        // OBJECT, which is exactly what `defining_class` alone encodes
        // (see `codegen::call::boxed_implicit_self`, which then yields
        // `RubyValue::Class(cid)` rather than an invalid `self.clone()`).
        current_class: None,
        defining_class: Some(cid),
        // A class body's `self` IS the class object, so a bare `@x = 1`
        // here is that class object's own ivar -- the same storage
        // `def self.x; @x; end` reads.
        class_self: Some(cid),
        current_method: None,
        current_method_origin: None,
        local_types: std::borrow::Cow::Borrowed(&no_locals),
        label_counter: &label_counter,
        loop_labels: None,
        next_yields_value: false,
        for_var_override: None,
        captured_locals: std::borrow::Cow::Borrowed(&captures.locals),
        binding_names,
        in_eval_splice: false,
        self_ident: format_ident!("self"),
        in_real_proc: false,
        self_is_dynamic: false,
        self_slots: false,
        shared_body: false,
        trace: None,
        runtime_super_params: None,
        defined_by_define_method: false,
        lexical_frame_label: None,
        block_depth: 0,
        has_blk_binding: false,
    };
    // Hoist the class body's own locals and emit its statements as a scoped
    // block that evaluates to `Result` -- `?` propagates a raised Signal into
    // the enclosing `run_main` closure, and the block scopes the locals so they
    // don't leak into `main_body` (a class body is its own scope in Ruby). The
    // `wrap_ok` tail is discarded by `?;` -- class-body statements run for
    // effect; `main_body` supplies the program's tail.
    let body = hoisting::emit_hoisted_body(&cx, stmts, true);
    // A class body executes under its own backtrace frame -- CRuby's
    // `<class:Foo>` / `<module:M>` (oracle-verified labels), initialized
    // at the site's own `class`/`module` keyword line, the first LOCATED
    // statement as fallback (aligned with `stamp_line`'s predicate -- see
    // `scope_frame_guard`). Fully span-less sites (prelude) skip it.
    let loc = site
        .def_node
        .and_then(|n| source_location(compiler, n))
        .or_else(|| stmts.iter().find_map(|&n| source_location(compiler, n)));
    let frame = match loc {
        Some((file, line)) => {
            let label = body_frame_label(compiler, cid);
            // The body's `end` line, `TracePoint`'s `:end` lineno.
            let end_line = site.def_node.map_or(0, |n| source_end_line(compiler, n));
            let file = pooled_file(file);
            quote! { let __frame = zeo_rt::FrameGuard::push(#file, #label, #line, #end_line); }
        }
        None => quote! {},
    };
    // `#alias_check` runs AFTER the body in both forms -- the value is bound
    // first so the ordering the statement form has survives into the value
    // one, where the body's result has to outlive the check.
    let inline_form = match value {
        BodyValue::Discard => {
            quote! { #const_location #const_added #inherited_hook { #frame #body }?; #alias_check }
        }
        // `tail_value` overrides the emitted body's own tail when analyze took
        // the last source statement, so what the block evaluates to is the
        // statement BEFORE it.
        _ => {
            let tail = tail_value(quote! { __body_value });
            quote! {
                {
                    #const_location #const_added #inherited_hook
                    let __body_value = { #frame #body }?;
                    #alias_check
                    #tail
                }
            }
        }
    };
    let Some(n) = lift else {
        return (quote! {}, inline_form);
    };
    debug_assert!(
        value == BodyValue::Discard,
        "only a hoisted site is lifted, and a hoisted site is never read for its value"
    );
    // A free `fn` closes over nothing, so a body that READS a local it does not
    // declare cannot be lifted -- it has to stay in its enclosing block.
    //
    // The shape is a guarded reopen whose guard was folded into the body:
    // activesupport's `duplicable.rb` writes `methods_are_duplicable = ...`
    // and then `unless methods_are_duplicable; class Method; ...; end; end`,
    // so the emitted body opens with a read of the FILE's local. Hoisted
    // sites were lifted unconditionally on the reasoning that they run at the
    // head of `run_main`, ahead of every top-level statement, and so have no
    // enclosing local to read. That is true of the main program and false
    // inside a `__unit_N`, where the site's own file-level locals are already
    // in scope -- rustc answered with `cannot find value
    // __f121_methods_are_duplicable in this scope`.
    //
    // Deciding it on the EMITTED tokens rather than re-deriving the body's
    // free names keeps the test and the thing tested identical: whatever the
    // body ended up reading is what gets checked.
    if !lift_is_closed(&body) {
        return (quote! {}, inline_form);
    }
    let ident = format_ident!("__class_body_{}", n);
    (
        // `inline(never)`: a `fn` with one call site is exactly what LLVM
        // inlines by default, which would put the body straight back into
        // `main` and undo the split. These run once, at class-definition
        // time, so nothing is lost by keeping them out of line.
        quote! {
            #[inline(never)]
            fn #ident() -> Result<zeo_rt::RubyValue, zeo_rt::Signal> { #frame #body }
        },
        quote! { #const_location #const_added #inherited_hook #ident()?; #alias_check },
    )
}

/// Whether `body` reads no local that it does not itself declare -- the
/// precondition for lifting it into a free `fn`, which closes over nothing.
///
/// Only zeo's own frame-mangled locals (`__f<frame>_<name>`, see
/// `hoisting::local_ident`) can be enclosing-scope reads; every other ident in
/// a generated body is a path, a type, or a `zeo_rt` item. So the test is
/// exactly: is every `__f*` ident used here also bound here?
///
/// A `let` binding is the only way one enters scope in generated code, and
/// `let mut x` / `let x` both put the name immediately after the keyword --
/// `mut` is skipped rather than treated as the binding.
fn lift_is_closed(body: &TokenStream) -> bool {
    fn walk(ts: &TokenStream, declared: &mut HashSet<String>, used: &mut Vec<String>) {
        let mut after_let = false;
        for tt in ts.clone() {
            match tt {
                proc_macro2::TokenTree::Ident(id) => {
                    let name = id.to_string();
                    if name == "let" {
                        after_let = true;
                        continue;
                    }
                    if after_let {
                        // `let mut x` -- the binding is the ident after `mut`.
                        if name != "mut" {
                            declared.insert(name);
                            after_let = false;
                        }
                        continue;
                    }
                    if name.starts_with("__f") {
                        used.push(name);
                    }
                }
                proc_macro2::TokenTree::Group(g) => {
                    after_let = false;
                    walk(&g.stream(), declared, used);
                }
                _ => after_let = false,
            }
        }
    }
    let mut declared = HashSet::new();
    let mut used = Vec::new();
    walk(body, &mut declared, &mut used);
    used.iter().all(|n| declared.contains(n))
}

/// The `const_added` a `class Foo` / `module M` fires for its OWN name, on the
/// lexically enclosing module. Only the FIRST of a class's sites declares
/// anything -- a reopen finds the constant already there and creates nothing,
/// the same rule `emit_inherited_hook` follows.
fn emit_declaration_const_added(
    compiler: &Compiler,
    site: &crate::compiler::ClassBodySite,
) -> TokenStream {
    let nil = quote! {};
    // A site with no source position is a synthetic registration -- the
    // built-in exception prelude, a pinned surrogate. Nothing declared those in
    // Ruby, and in CRuby they exist before the program's first statement runs.
    if site.def_node.is_none() || !declares_the_class(compiler, site) {
        return nil;
    }
    let owner = declaration_owner(compiler, site.class);
    emit_const_added(
        compiler,
        owner,
        compiler.leaf_name(site.class),
        site.def_node,
    )
}

/// Whether this site is the one that CREATES the class -- a reopen finds the
/// constant already there and declares nothing.
fn declares_the_class(compiler: &Compiler, site: &crate::compiler::ClassBodySite) -> bool {
    compiler
        .class_body_sites
        .iter()
        .find(|s| s.class == site.class)
        .is_some_and(|s| std::ptr::eq(s, site))
}

/// The module a class declaration binds its own name in.
fn declaration_owner(
    compiler: &Compiler,
    cid: crate::compiler::ClassId,
) -> crate::compiler::ClassId {
    compiler
        .class(cid)
        .lexical_parent
        .unwrap_or(crate::compiler::OBJECT_CLASS)
}

/// Where `class Foo` / `module M` binds its own name, for
/// `Module#const_source_location`. CRuby stamps this when the constant is
/// CREATED, so a reopen leaves the first declaration's line standing -- the
/// same first-site rule `emit_declaration_const_added` follows.
fn emit_declaration_const_location(
    compiler: &Compiler,
    site: &crate::compiler::ClassBodySite,
) -> TokenStream {
    let nil = quote! {};
    if !declares_the_class(compiler, site) {
        return nil;
    }
    let Some((file, line)) = site.def_node.and_then(|n| source_location(compiler, n)) else {
        return nil;
    };
    let owner = declaration_owner(compiler, site.class).0;
    let name = compiler.leaf_name(site.class);
    quote! { zeo_rt::record_const_location(#owner, #name, #file, #line); }
}

/// Whether the hook body `hook` was already installed at position `at`.
///
/// A hook INSTALLED after the thing it would report never saw it. minitest
/// reopens `Runnable` at the very end of its main file purely to add
/// `inherited`, so that the `Test`/`Result` subclasses defined above stay out
/// of the runnables registry -- fire it for them and `Result`, which implements
/// no `runnable_methods`, joins the run and raises.
///
/// Compared by SPAN, and only within one file: `doc_order` numbers class-body
/// statements, and a `def` is not one of those (it is hoisted into the class's
/// method table). Two positions in different files are left alone -- a spliced
/// `require` puts another file's statements in the middle of this one, so raw
/// offsets do not order across files -- as is anything span-less. All of those
/// keep firing.
fn hook_installed_before(
    compiler: &Compiler,
    hook: crate::compiler::ScopeId,
    at: Option<crate::hir::NodeId>,
) -> bool {
    let where_ = |n: Option<crate::hir::NodeId>| {
        n.and_then(|n| compiler.hir.span(n)).and_then(|s| s.known())
    };
    match (where_(compiler.scope(hook).def_node), where_(at)) {
        (Some(installed), Some(at)) if installed.file == at.file => installed.start <= at.start,
        _ => true,
    }
}

/// `Module#const_added` -- Ruby announces a constant the moment it becomes
/// readable, on the module it was set on, with its own leaf name. Emits
/// nothing unless the owner's chain defines the hook (`Module`'s default is a
/// no-op), so a program without one is unchanged.
pub(crate) fn emit_const_added(
    compiler: &Compiler,
    owner: crate::compiler::ClassId,
    name: &str,
    at: Option<crate::hir::NodeId>,
) -> TokenStream {
    let nil = quote! {};
    // A `class Module; def const_added` reopen answers for every module, and no
    // per-class scan can see it -- see `Compiler::global_def_hooks`.
    if !compiler.global_def_hooks.contains("const_added") {
        let Some((_, hook)) = compiler.class_method_in_chain(owner, "const_added") else {
            return nil;
        };
        if !hook_installed_before(compiler, hook, at) {
            return nil;
        }
    }
    let o = owner.0;
    let (sym, arg) = (pooled_sym("const_added"), pooled_sym(name));
    quote! {
        zeo_rt::send_value(
            &zeo_rt::RubyValue::Class(zeo_rt::ClassId(#o)),
            #sym,
            &[zeo_rt::RubyValue::Symbol(#arg)],
            None,
        )?;
    }
}

/// `Class#inherited` -- Ruby runs `Super.inherited(C)` when the class is
/// CREATED, so only the FIRST of a class's sites fires it; a reopen creates
/// nothing. Emits nothing unless the superclass chain defines the hook
/// (`Class`'s own default is a no-op) or the class has no superclass at all.
fn emit_inherited_hook(compiler: &Compiler, site: &crate::compiler::ClassBodySite) -> TokenStream {
    let nil = quote! {};
    let cid = site.class;
    let first = compiler
        .class_body_sites
        .iter()
        .find(|s| s.class == cid)
        .is_some_and(|s| std::ptr::eq(s, site));
    if !first {
        return nil;
    }
    let Some(parent) = compiler.class(cid).parent else {
        return nil;
    };
    let Some((_, hook)) = compiler.class_method_in_chain(parent, "inherited") else {
        return nil;
    };
    if !hook_installed_before(compiler, hook, site.def_node) {
        return nil;
    }
    let (p, c) = (parent.0, cid.0);
    let sym = pooled_sym("inherited");
    quote! {
        zeo_rt::send_value(
            &zeo_rt::RubyValue::Class(zeo_rt::ClassId(#p)),
            #sym,
            &[zeo_rt::RubyValue::Class(zeo_rt::ClassId(#c))],
            None,
        )?;
    }
}

/// The `ClassDef` markers whose sites execute INLINE, in document order:
/// everything reachable from the top-level statement stream -- directly, a
/// statement inside a `BoxScope` splice, or nested inside another
/// reachable site's own body. The complement (prelude bootstrap sites,
/// synthetic `None`-marker registrations) keeps the hoisted splice.
fn inline_class_markers(
    compiler: &Compiler,
    main_statements: &[crate::hir::NodeId],
) -> HashSet<crate::hir::NodeId> {
    let site_by_marker: HashMap<crate::hir::NodeId, &crate::compiler::ClassBodySite> = compiler
        .class_body_sites
        .iter()
        .filter_map(|s| s.def_node.map(|n| (n, s)))
        .collect();
    let mut seen = HashSet::new();
    let mut work: Vec<crate::hir::NodeId> = main_statements.to_vec();
    while let Some(s) = work.pop() {
        match &compiler.hir[s] {
            crate::hir::HirNode::ClassDef { .. } => {
                if seen.insert(s)
                    && let Some(site) = site_by_marker.get(&s)
                {
                    work.extend(site.stmts.iter().copied());
                }
            }
            crate::hir::HirNode::BoxScope { body, .. } => work.extend(body.iter().copied()),
            // Every other statement container -- a `begin`, an `if`, a `case`,
            // a loop, a block. A class written inside one runs where it is
            // written, which for a block means each time the block runs. Only
            // a `def`'s body is a separate function that waits to be called,
            // so that is where the walk stops.
            crate::hir::HirNode::DefMethod { .. } => {}
            other => other.for_each_child(&mut |c| work.push(c)),
        }
    }
    seen
}

/// Class methods (`def self.x`, `extend`) -- see `ClassInfo::class_methods`'s
/// docs. A plain, ADDITIONAL `impl #name { }` block for a class (Rust
/// allows more than one `impl Type { }` for the same type, so this doesn't
/// conflict with `ruby_class!`'s own), or a `pub mod #name { }` of free
/// functions for a module (no struct to attach an `impl` to at all) --
/// either way, called identically at the call site (`#name::#method(...)`,
/// see `codegen::call`'s `ClassRef` handling), so nothing downstream needs
/// to know which kind of container it is.
///
/// Full `Params` support (same signature/prologue machinery as
/// instance methods, receiverless). **Remaining scope-cut**: a class
/// method's own body may not reference `self`/`@ivar` -- there's no
/// concrete instance for `self` to mean here (a class-level ivar /
/// `class << self` state store is not attempted yet).
fn emit_class_methods(compiler: &Compiler, cid: ClassId) -> TokenStream {
    let ci = compiler.class(cid);
    let name_ident = ident::class_ident(compiler, cid);
    let fns = ci
        .class_methods
        .iter()
        .map(|e| emit_class_method_fn(compiler, e.owner, e.def));
    // A container without a generated struct to attach an `impl` to -- a module,
    // OR a native-backed class (`RubyException`/`ValueSubclass`), OR an immediate
    // subclass (registry-only) -- emits `def self.x` into a `pub mod` of free
    // functions, reached at the call site as `#name::x(...)` like a module's.
    // (`emit_class_methods` is only called for user classes/modules, never
    // builtins, so `!has_generated_struct` cleanly means "structless".)
    if !compiler.has_generated_struct(cid) {
        // Ruby module names are conventionally PascalCase (matching a Rust
        // struct/type's own convention), which `rustc` otherwise flags as
        // non-idiomatic for a `mod` (conventionally snake_case) -- silenced
        // rather than renamed, since call sites (`ModuleName::method(...)`)
        // must match the Ruby-visible name exactly.
        // `use super::*;`: unlike a class's `impl` block (whose function
        // bodies resolve paths at the CRATE root, where every generated
        // item lives), a `pub mod` is a real child module -- a module
        // function's body referencing any sibling top-level item (another
        // module's functions, a class's `new_handle` for `SomeClass.new`/
        // `raise`) wouldn't resolve without re-importing the root scope.
        // Found by a cross-package test (`Greet.hi` calling
        // `Upper.dashed`), but reproducible in one file with any
        // module-function calling another module -- a pre-existing gap.
        quote! {
            #[allow(non_snake_case)]
            pub mod #name_ident { #[allow(unused_imports)] use super::*; #(#fns)* }
        }
    } else {
        quote! { impl #name_ident { #(#fns)* } }
    }
}

/// `owner` is the class this copy is emitted INTO -- CRuby's `owner`, and what
/// class-level ivar storage keys on, so `Sub.reg` reads Sub's slot even though
/// the body came from Base. It is passed in rather than read off the scope
/// because one definition now serves every class that inherited it.
fn emit_class_method_fn(
    compiler: &Compiler,
    owner: ClassId,
    sid: crate::compiler::ScopeId,
) -> TokenStream {
    let scope = compiler.scope(sid);
    let params = &scope.params;
    let needs_block = scope.needs_block_param();
    // Mangled: instance and class methods share one generated container, so
    // a class defining both `def x` and `def self.x` (two namespaces in
    // Ruby, ordinary code) would otherwise emit two `fn x` into the same
    // `impl`. See `ident::class_method_ident`'s docs.
    let method_ident = ident::class_method_ident(&scope.name);
    let sig_params = params::emit_signature_params_free(params, needs_block);
    let label_counter = Cell::new(0u32);
    let mut no_captures = captures::collect_escaping_captures(
        compiler,
        &scope.body,
        &scope.params,
        class_query::SelfClass::default(),
    );
    let binding_names = captures::binding_scope_names(
        compiler,
        &scope.body,
        &scope.params,
        &mut no_captures,
        false,
    );
    // A def from a constant-bearing `class << self` body is lexically inside
    // the SINGLETON class: its bare constants and `Module.nesting` resolve
    // through the surrogate (whose lexical parent is the class itself, so
    // everything else in the chain is unchanged). See `Scope::lexical_home`.
    let lexical = scope.lexical_home.unwrap_or(scope.defining_class);
    let cx = Ctx {
        compiler,
        box_id: compiler.class(lexical).box_id,
        // No concrete receiver exists for a class method (no `self:
        // Arc<Self>`) -- but `defining_class` (which class/module this body
        // was LEXICALLY written in) still needs to be real, for `@@cvar`
        // ownership lookup (`codegen::expr::cvar_owner_id`).
        current_class: None,
        defining_class: Some(lexical),
        // The OWNER -- which class this body is emitted into -- deliberately
        // NOT `defining_class` (where it was written). The two differ exactly
        // when a class method is inherited, and that is precisely the case
        // class-ivar storage must tell apart: `Sub.reg` reads Sub's slot even
        // though the body came from Base. See `Ctx::class_self`'s docs.
        class_self: Some(owner),
        current_method: Some(scope.name.clone()),
        current_method_origin: scope.alias_of.clone(),
        defined_by_define_method: scope_is_define_method(compiler, scope),
        lexical_frame_label: None,
        local_types: binding_scope_local_types(
            binding_names.as_ref(),
            &no_captures.locals,
            &scope.local_types,
        ),
        label_counter: &label_counter,
        loop_labels: None,
        next_yields_value: false,
        for_var_override: None,
        captured_locals: std::borrow::Cow::Borrowed(&no_captures.locals),
        binding_names,
        in_eval_splice: false,
        self_ident: format_ident!("self"),
        in_real_proc: false,
        self_is_dynamic: false,
        self_slots: false,
        shared_body: false,
        trace: None,
        runtime_super_params: None,
        block_depth: 0,
        has_blk_binding: needs_block,
    };
    let prologue = params::emit_prologue(&cx, &scope.params, &scope.body);
    let body = hoisting::emit_hoisted_body_with_extra_roots(
        &cx,
        &scope.body,
        &scope.params.default_ids(),
        &scope.params.bound_names(),
        true,
    );
    let body = quote! { #prologue #body };
    // See the matching comment on `emit_class`'s own method-wrapping below:
    // a `begin`/`rescue` construct (or an escaping block, e.g. `arr.each { ...
    // return ... }` -- this function only rejects a class method that itself
    // NAMES a `&block` param/uses bare `yield`, not one that merely calls
    // something with a literal block argument) introduces its own closure
    // boundary a literal `return` can't cross, so this class method's body
    // needs the SAME per-method `Signal::Return` catch an ordinary instance
    // method gets when it contains either.
    let needs_return_catch = captures::body_contains_begin(compiler, &scope.body)
        || captures::body_contains_escaping_return(compiler, &scope.body);
    let body_tokens = wrap_method_return(needs_return_catch, body);
    let frame = scope_frame_guard(compiler, scope, true);
    // A class method's traced `self` is the class object itself -- the OWNER,
    // which is the class the call actually landed on.
    let self_note = {
        let id = owner.0;
        quote! {
            zeo_rt::trace_frame_self(|| zeo_rt::RubyValue::Class(zeo_rt::ClassId(#id)));
        }
    };
    // `check_ints` after the frame push: the method-prologue interruption
    // checkpoint (pairs with the back-edge check in `loops`), so recursion-
    // driven busy work is killable even with no native loop in sight.
    quote! {
        #[allow(unused_variables)]
        pub fn #method_ident(#sig_params) -> Result<zeo_rt::RubyValue, zeo_rt::Signal> {
            #frame
            #self_note
            zeo_rt::check_ints()?;
            #body_tokens
        }
    }
}

/// The generated container for a REOPENED builtin class -- a
/// `pub mod __bm_<Name>` (see `ident::class_ident`'s builtin mangling for
/// why not a bare `pub mod String`) holding the reopen's instance methods
/// as FREE FUNCTIONS (`__self: RubyValue` first parameter -- a builtin has
/// no struct for a `self: Arc<Self>` receiver) and its `def self.x` class
/// methods (the same `emit_class_method_fn` shape a module container gets).
/// Both kinds share one module, so one name can't be both -- a clean panic
/// rather than a confusing generated-`rustc` duplicate-definition error.
/// User-module method bridges: for each `module Foo; def bar; end` the compiler
/// otherwise ONLY materializes into includers (a module gets no dispatch struct
/// and its methods aren't retrievable by id). To make `obj.extend(Foo)` work at
/// runtime -- which needs Foo's method impls BY ID -- emit each user module's
/// own methods into a dedicated `__um_<id>_<Name>` container of `RubyValue`-self
/// free functions and register them as value methods on the module's id, the
/// same shape builtin modules (`Comparable`) use. Include is untouched (it still
/// materializes off `own_methods` at compile time). The registrations ride
/// `__VM_ROWS`, applied after every `__registry.register` (each entry must
/// exist first -- `define_value_method` asserts it).
fn emit_user_module_bridges(compiler: &Compiler) -> Vec<TokenStream> {
    let mut containers = Vec::new();
    for (idx, class) in compiler.classes.iter().enumerate() {
        if idx == 0 || class.is_builtin || class.is_bootstrap || !class.is_module {
            continue;
        }
        if class.own_methods.is_empty() || !compiler.feature_active(ClassId(idx as u32)) {
            continue;
        }
        let cid = ClassId(idx as u32);
        let id = idx as u32;
        // The name only spells the container for a human reader, so anything
        // that isn't a Rust ident character becomes `_` -- a refinement
        // holder (`#refinement:String`) is deliberately unspellable as a
        // Ruby constant and would otherwise not be an ident at all.
        let flat: String = class
            .name
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect();
        let container = format_ident!("__um_{}_{}", idx, flat);
        let fns = class
            .own_methods
            .iter()
            .map(|&sid| emit_builtin_method_fn(compiler, cid, sid));
        containers.push(quote! {
            #[allow(non_snake_case)]
            pub mod #container { #[allow(unused_imports)] use super::*; #(#fns)* }
        });
        for &sid in &class.own_methods {
            let scope = compiler.scope(sid);
            let method_ident = safe_ident(&scope.name);
            let fn_path = quote! { #container::#method_ident };
            let tramp = params::emit_value_trampoline(
                &fn_path,
                &scope.name,
                &scope.params,
                scope.needs_block_param(),
                params::RecvMode::Pass,
                &scope_frame_guard(compiler, scope, false),
            );
            push_vm_row(id, 0, &scope.name, tramp);
            // A module declares its methods in `own_methods`, while the
            // registration loop's visibility pass reads the materialized
            // `methods` -- empty for a module nothing includes. So the mark
            // goes where the ROW goes, and `private` inside a module body
            // finally reaches the registry.
            match scope.visibility {
                crate::hir::Visibility::Private => push_vis_row(id, &scope.name, 0),
                crate::hir::Visibility::Protected => push_vis_row(id, &scope.name, 1),
                crate::hir::Visibility::Public => {}
            }
        }
    }
    containers
}

fn emit_builtin_reopen(
    compiler: &Compiler,
    shared: &share::SharedBodies,
    cid: ClassId,
) -> TokenStream {
    let ci = compiler.class(cid);
    let mod_ident = ident::class_ident(compiler, cid);
    let instance_fns = ci.methods.iter().map(|e| {
        // A body this class shares with other structless receivers lives once
        // in `__sh`; the container keeps a forwarder under the real name so
        // every path that names `__bm_Foo::bar` -- dispatch rows, sibling
        // implicit-self calls, `module_function` registrations -- is unchanged.
        // The receiver is a `RubyValue` on both sides, so the forwarder is a
        // straight hand-off with nothing to box or unbox.
        match shared.call(cid, e.def) {
            Some(shared_fn) => emit_builtin_forwarder(compiler, e.def, shared_fn),
            None => emit_builtin_method_fn(compiler, cid, e.def),
        }
    });
    // Instance and class methods share this one container but not their
    // idents (`x` vs `__cm_x`), so a reopen defining both -- `module Kernel;
    // def URI(u); end; module_function :URI; end`, which is what
    // `module_function` produces -- emits cleanly.
    let class_fns = ci
        .class_methods
        .iter()
        .map(|e| emit_class_method_fn(compiler, e.owner, e.def));
    // `use super::*;` for the same reason a module's class-method container
    // needs it (see `emit_class_methods`): a `pub mod` is a real child
    // module, and these bodies reference sibling top-level items.
    quote! {
        #[allow(non_snake_case)]
        pub mod #mod_ident { #[allow(unused_imports)] use super::*; #(#instance_fns)* #(#class_fns)* }
    }
}

/// A structless class's stand-in for a body that now lives once in `__sh`.
///
/// The container's function keeps the method's real name and signature, so
/// every path that already names `__bm_Foo::bar` keeps working; the body is one
/// hand-off. Both sides take `__self: zeo_rt::RubyValue`, which is the whole
/// reason this is free -- there is no receiver to box, unbox or coerce, unlike
/// the struct-backed forwarder in `emit_class` which has to consume its
/// `Arc<Self>`.
///
/// No frame guard, no `check_ints`: the shared body pushes its own, and pushing
/// a second here would put a duplicate entry in every backtrace.
fn emit_builtin_forwarder(
    compiler: &Compiler,
    sid: crate::compiler::ScopeId,
    shared_fn: &proc_macro2::Ident,
) -> TokenStream {
    let scope = compiler.scope(sid);
    let method_ident = safe_ident(&scope.name);
    let needs_block = scope.needs_block_param();
    let sig_params = params::emit_signature_params(&scope.params, needs_block);
    let fwd = params::emit_forward_args(&scope.params, needs_block);
    quote! {
        #[allow(unused_variables)]
        pub fn #method_ident(__self: zeo_rt::RubyValue #sig_params) -> Result<zeo_rt::RubyValue, zeo_rt::Signal> {
            __sh::#shared_fn(__self #fwd)
        }
    }
}

/// The native-exception reopen/subclass DELTA: for an exception-backed
/// class (`is_exception_backed`), the methods whose winning body is NOT a
/// pristine `BUILTIN_EXCEPTIONS_RB` body (`!native_default`) and that were
/// actually defined ON an exception class (never a top-level `def`
/// materialized from `Object`). Returns a `__exc_<id>` container of
/// `RubyValue`-self free functions (`emit_builtin_method_fn`, name-keyed ivars)
/// plus `define_method` registrations that layer over -- replacing, for an
/// override -- the native defaults `register_exceptions` already installed on
/// this class's id. Empty (`None`) when the class carries no user deltas (the
/// pure-native case, or a subclass that adds nothing).
fn emit_exception_deltas(
    compiler: &Compiler,
    cid: ClassId,
) -> Option<(TokenStream, Vec<TokenStream>)> {
    let deltas: Vec<crate::compiler::ScopeId> = compiler
        .class(cid)
        .methods
        .iter()
        .filter(|e| {
            !e.native_default && compiler.is_native_backed(compiler.scope(e.def).defining_class)
        })
        .map(|e| e.def)
        .collect();
    if deltas.is_empty() {
        return None;
    }
    let mod_ident = format_ident!("__exc_{}", cid.0);
    let fns = deltas
        .iter()
        .map(|&sid| emit_builtin_method_fn(compiler, cid, sid));
    let container = quote! {
        #[allow(non_snake_case)]
        pub mod #mod_ident { #[allow(unused_imports)] use super::*; #(#fns)* }
    };
    let id = cid.0;
    let regs = deltas
        .iter()
        .map(|&sid| {
            let scope = compiler.scope(sid);
            let name = &scope.name;
            let method_ident = safe_ident(name);
            let fn_path = quote! { #mod_ident::#method_ident };
            let frame = scope_frame_guard(compiler, scope, false);
            // A `class X < Module` instance is a `RubyValue::Class`, not an
            // `RObj`: `emit_exc_trampoline` would downcast it to
            // `RubyException` and miss. The value-shaped trampoline passes the
            // receiver straight through to the same
            // `emit_builtin_method_fn` body, which already takes a `RubyValue`
            // self -- so only the REGISTRATION differs.
            if compiler.is_module_subclass(cid) {
                let tramp = params::emit_value_trampoline(
                    &fn_path,
                    name,
                    &scope.params,
                    scope.needs_block_param(),
                    params::RecvMode::Pass,
                    &frame,
                );
                return quote! {
                    __registry.define_value_method(
                        zeo_rt::ClassId(#id),
                        0,
                        zeo_rt::Symbol::intern(#name),
                        #tramp,
                    );
                };
            }
            let tramp = params::emit_exc_trampoline(
                &fn_path,
                name,
                &scope.params,
                scope.needs_block_param(),
                &frame,
            );
            quote! {
                __registry.define_method(
                    zeo_rt::ClassId(#id),
                    zeo_rt::Symbol::intern(#name),
                    #tramp,
                );
            }
        })
        .collect();
    Some((container, regs))
}

/// One reopened-builtin INSTANCE method -- the free-function
/// counterpart of `emit_class`'s per-method emission: same params
/// machinery, same hoisting, same `Signal::Return` catch rule; the receiver
/// is the `__self: RubyValue` first parameter (`Ctx::self_ident` points at
/// it, so `SelfRef`/interpolation/`yield` all compose unchanged), and
/// `current_class` carries the builtin's id so `self` types as the builtin
/// kind (see `expr::builtin_self_ty`) and implicit-self calls resolve
/// against the reopen's own methods.
fn emit_builtin_method_fn(
    compiler: &Compiler,
    cid: ClassId,
    sid: crate::compiler::ScopeId,
) -> TokenStream {
    let method_ident = safe_ident(&compiler.scope(sid).name);
    // A reopened builtin has no generated struct, so its ivars are name-keyed
    // with nowhere for a slot index to point.
    emit_value_self_method_fn(compiler, cid, sid, &method_ident, false, false, None)
}

/// `emit_builtin_method_fn` with the function's own name supplied, since a
/// SHARED body (`codegen::share`) is emitted under a group name rather than the
/// method's. The receiver is a `RubyValue` either way, which is what makes one
/// body servable by classes with different concrete structs.
///
/// `trace`, when present, collects every question this emission asks about
/// `cid` -- see [`class_query`]. `share` records one member's body and then
/// replays the trace against the rest instead of emitting them at all.
///
/// `self_slots` and `shared` are two DIFFERENT questions, and reading one off
/// the other was a bug. `self_slots` asks whether the receiver has an ivar
/// layout to index into; `shared` asks whether this body serves more than one
/// class, which is what suppresses the per-site inline cache (a `CallSite`
/// index differs between two emissions, so a body carrying one can never match
/// its group). They agree for a struct-backed shared body, which is why tying
/// them held until reopened builtins started sharing -- there `self_slots` is
/// false and `shared` is true, and `ZEO_VERIFY_SHARE` caught the mismatch as
/// `Object#DelegateClass` emitting `send_value_cached` in one member and
/// `send_value_explicit_in` in another.
fn emit_value_self_method_fn(
    compiler: &Compiler,
    cid: ClassId,
    sid: crate::compiler::ScopeId,
    method_ident: &proc_macro2::Ident,
    self_slots: bool,
    shared: bool,
    trace: Option<&std::cell::RefCell<class_query::Trace>>,
) -> TokenStream {
    let scope = compiler.scope(sid);
    let needs_block = scope.needs_block_param();
    let sig_params = params::emit_signature_params(&scope.params, needs_block);
    let label_counter = Cell::new(0u32);
    let mut method_captures = captures::collect_escaping_captures(
        compiler,
        &scope.body,
        &scope.params,
        class_query::SelfClass::new(Some(cid), trace),
    );
    let binding_names = captures::binding_scope_names(
        compiler,
        &scope.body,
        &scope.params,
        &mut method_captures,
        false,
    );
    let cx = Ctx {
        compiler,
        box_id: compiler.class(scope.defining_class).box_id,
        current_class: Some(cid),
        defining_class: Some(scope.defining_class),
        // An INSTANCE method: `self` is an instance of `cid`, not the class
        // object -- so `@x` here is the instance's own field, not class-level
        // storage.
        class_self: None,
        current_method: Some(scope.name.clone()),
        current_method_origin: scope.alias_of.clone(),
        defined_by_define_method: scope_is_define_method(compiler, scope),
        lexical_frame_label: None,
        local_types: binding_scope_local_types(
            binding_names.as_ref(),
            &method_captures.locals,
            &scope.local_types,
        ),
        label_counter: &label_counter,
        loop_labels: None,
        next_yields_value: false,
        for_var_override: None,
        captured_locals: std::borrow::Cow::Borrowed(&method_captures.locals),
        binding_names,
        in_eval_splice: false,
        self_ident: format_ident!("__self"),
        in_real_proc: false,
        // `__self` here is the `RubyValue` receiver parameter, not an
        // `Arc<Concrete>` -- a reopened builtin has no generated struct to
        // take ivar fields from, and for an `Object` reopen (which is where
        // TOP-LEVEL `def`s live) the receiver is the `main` object, whose
        // ivars are name-keyed with storage in `dispatch::Object`.
        self_is_dynamic: true,
        self_slots,
        shared_body: shared,
        trace,
        runtime_super_params: None,
        block_depth: 0,
        has_blk_binding: needs_block,
    };
    let prologue = params::emit_prologue(&cx, &scope.params, &scope.body);
    let body = hoisting::emit_hoisted_body_with_extra_roots(
        &cx,
        &scope.body,
        &scope.params.default_ids(),
        &scope.params.bound_names(),
        true,
    );
    // Same needs-a-`Signal::Return`-catch rule as `emit_class`'s methods --
    // see the long comment there.
    let needs_return_catch = captures::body_contains_escaping_return(compiler, &scope.body)
        || captures::body_contains_begin(compiler, &scope.body);
    let body_tokens = wrap_method_return(needs_return_catch, quote! { #prologue #body });
    let frame = scope_frame_guard(compiler, scope, false);
    // `allow(unused_variables)`: a reopen method that never references
    // `self` leaves `__self` unread -- unlike a real `self` receiver
    // parameter, which rustc never warns about.
    quote! {
        #[allow(unused_variables)]
        pub fn #method_ident(__self: zeo_rt::RubyValue #sig_params) -> Result<zeo_rt::RubyValue, zeo_rt::Signal> {
            #frame
            zeo_rt::trace_frame_self(|| __self.clone());
            zeo_rt::check_ints()?;
            #body_tokens
        }
    }
}

/// One instance method's emitted interior, split at the point where the
/// wrapper differs: `emit_class` puts it inside a `ruby_class!` `def` with a
/// `self: Arc<Self>` receiver, while a shared body puts the same tokens inside
/// a free function. Everything above the receiver -- captures, `Ctx`, prologue,
/// hoisting, the return catch, the frame -- is identical either way, which is
/// exactly the property `analyze::share` has to prove.
pub(crate) struct InstanceMethodBody {
    /// The frame guard and `check_ints`, absent for an accessor.
    pub preamble: Option<TokenStream>,
    pub body: TokenStream,
}

/// Emit `sid`'s body as it looks when `cid` is the receiver's class.
pub(crate) fn emit_instance_method_body(
    compiler: &Compiler,
    cid: ClassId,
    sid: crate::compiler::ScopeId,
) -> InstanceMethodBody {
    let scope = compiler.scope(sid);
    let needs_block = scope.needs_block_param();
    let method_label_counter = Cell::new(0u32);
    let mut method_captures = captures::collect_escaping_captures(
        compiler,
        &scope.body,
        &scope.params,
        class_query::SelfClass::new(Some(cid), None),
    );
    let binding_names = captures::binding_scope_names(
        compiler,
        &scope.body,
        &scope.params,
        &mut method_captures,
        false,
    );
    let method_cx = Ctx {
        compiler,
        box_id: compiler.class(scope.defining_class).box_id,
        current_class: Some(cid),
        defining_class: Some(scope.defining_class),
        // An instance method -- see the matching note in `emit_method_fn`.
        class_self: None,
        current_method: Some(scope.name.clone()),
        current_method_origin: scope.alias_of.clone(),
        defined_by_define_method: scope_is_define_method(compiler, scope),
        lexical_frame_label: None,
        local_types: binding_scope_local_types(
            binding_names.as_ref(),
            &method_captures.locals,
            &scope.local_types,
        ),
        label_counter: &method_label_counter,
        loop_labels: None,
        next_yields_value: false,
        for_var_override: None,
        captured_locals: std::borrow::Cow::Borrowed(&method_captures.locals),
        binding_names,
        in_eval_splice: false,
        self_ident: format_ident!("self"),
        in_real_proc: false,
        self_is_dynamic: false,
        self_slots: false,
        shared_body: false,
        trace: None,
        runtime_super_params: None,
        block_depth: 0,
        has_blk_binding: needs_block,
    };
    let prologue = params::emit_prologue(&method_cx, &scope.params, &scope.body);
    let body = hoisting::emit_hoisted_body_with_extra_roots(
        &method_cx,
        &scope.body,
        &scope.params.default_ids(),
        &scope.params.bound_names(),
        true,
    );
    // The `Signal::Return` catch is needed ONLY when this method's OWN
    // body lexically contains a `return` INSIDE an escaping block OR a
    // `begin`/`rescue` construct -- confirmed the hard way NOT to be "wrap
    // every method unconditionally", nor even "any escaping block": a pure
    // relay (e.g. one that just does `yield` to whatever block it's handed,
    // even while holding its own return-less escaping block) must NOT catch
    // `Signal::Return` in transit, or it would incorrectly intercept a
    // `return` meant for a DIFFERENT method -- wherever the block it's
    // currently invoking was actually written -- turning "return from the
    // caller" into "this method returns normally instead". See
    // `codegen::captures::body_contains_escaping_return`'s docs, and
    // `codegen::exceptions`'s module docs for why `begin`/`rescue` ALSO
    // needs this (it introduces its own closure boundary a literal
    // `return` can't cross either).
    let needs_return_catch = captures::body_contains_escaping_return(compiler, &scope.body)
        || captures::body_contains_begin(compiler, &scope.body);
    let body = wrap_method_return(needs_return_catch, quote! { #prologue #body });
    // An `attr_*` accessor gets NO frame, because CRuby's does not either:
    // it compiles them iseq-less, so they appear in no backtrace -- a
    // `FrozenError` from `attr_writer` reports only the CALLER's line, and
    // an arity error likewise (oracle-verified both ways against a
    // hand-written `def x=(v); @x = v; end`, which does get its frame).
    // A hand-written accessor keeps its frame whenever anything could
    // observe one: a reader's body cannot raise and calls nothing, so only
    // `TracePoint` could tell, but a writer's frozen guard raises and its
    // backtrace must name it.
    let frameless = compiler
        .accessor_shape(cid, scope)
        .is_some_and(|a| a.attr_generated || a.kind == crate::compiler::AccessorKind::Reader);
    // Dropping `check_ints` with it cannot make a program uninterruptible:
    // an accessor body is a leaf, and every loop and every block already
    // checks on each iteration (`codegen::loops`, `codegen::call::procs`).
    let preamble = (!frameless).then(|| {
        let frame = scope_frame_guard(compiler, scope, false);
        // The armed-only `TracePoint#self` note: boxes the receiver ONLY
        // while a trace hook is on (one relaxed load otherwise). UFCS
        // `Arc::clone`, never `self.clone()` -- a user method named `clone`
        // would shadow the handle bump.
        quote! {
            #frame
            zeo_rt::trace_frame_self(
                || zeo_rt::RubyValue::Object(Self::new_handle(std::sync::Arc::clone(&self))),
            );
            zeo_rt::check_ints()?;
        }
    });
    InstanceMethodBody { preamble, body }
}

/// The Rust ident a SUPERSEDED redefinition body is emitted under -- the
/// live body owns the plain `safe_ident` name, so older bodies mangle with
/// their own `ScopeId` (globally unique).
pub(crate) fn redef_ident(sid: crate::compiler::ScopeId, name: &str) -> proc_macro2::Ident {
    let base = safe_ident(name);
    format_ident!("__r{}_{}", sid.0, base)
}

/// The `MethodFn` trampoline that installs scope `sid` as `cid`'s current
/// body of its name -- shared by the boot install of the first body
/// (`Compiler::positional_redefs`) and each positional re-install
/// (`HirNode::MethodRedefine`). It calls the body's free-function emission
/// in the class's `__redef_<id>` container (see `emit_redef_containers`):
/// `RubyValue`-self with name-keyed ivars, because the overlay entry
/// propagates down the ancestry and a SUBCLASS instance is a different
/// concrete struct than the defining class's -- a downcasting inherent-
/// method trampoline would panic on it.
pub(crate) fn redef_trampoline(
    compiler: &Compiler,
    cid: ClassId,
    sid: crate::compiler::ScopeId,
) -> TokenStream {
    let container = redef_container_ident(cid);
    let scope = compiler.scope(sid);
    let method_ident = redef_ident(sid, &scope.name);
    let fn_path = quote! { #container::#method_ident };
    params::emit_exc_trampoline(
        &fn_path,
        &scope.name,
        &scope.params,
        scope.needs_block_param(),
        &scope_frame_guard(compiler, scope, false),
    )
}

fn redef_container_ident(cid: ClassId) -> proc_macro2::Ident {
    format_ident!("__redef_{}", cid.0)
}

/// One container module per class with an observable redefinition timeline
/// (`ClassInfo::redef_scopes`): every body of the redefined name -- the
/// superseded ones AND the final one -- as `RubyValue`-self free functions
/// for `redef_trampoline` to install.
fn emit_redef_containers(compiler: &Compiler) -> Vec<TokenStream> {
    let mut containers = Vec::new();
    for (idx, class) in compiler.classes.iter().enumerate() {
        if class.redef_scopes.is_empty() {
            continue;
        }
        let cid = ClassId(idx as u32);
        let container = redef_container_ident(cid);
        let fns = class.redef_scopes.iter().map(|&sid| {
            let ident = redef_ident(sid, &compiler.scope(sid).name);
            emit_value_self_method_fn(compiler, cid, sid, &ident, false, false, None)
        });
        containers.push(quote! {
            #[allow(non_snake_case)]
            pub mod #container { #[allow(unused_imports)] use super::*; #(#fns)* }
        });
    }
    containers
}

fn emit_class(compiler: &Compiler, shared: &share::SharedBodies, cid: ClassId) -> TokenStream {
    let ci = compiler.class(cid);
    let name_ident = ident::class_ident(compiler, cid);
    // The registry's Ruby-visible name: fully qualified, so
    // `puts Store::Item` and NoMethodError messages print the real path.
    let fq_name = compiler.fq_name(cid);
    let parent = ci.parent.unwrap_or(OBJECT_CLASS);
    // A BUILTIN parent (`< Struct`) has no generated Rust
    // struct -- structurally the class sits on `Object`; the SEMANTIC
    // chain (ancestors, is_a?, rescue) carries the real parent.
    let parent_ty = if parent == OBJECT_CLASS || compiler.class(parent).is_builtin {
        quote! { zeo_rt::Object }
    } else {
        let parent_ident = ident::class_ident(compiler, parent);
        quote! { #parent_ident }
    };
    let id = cid.0;
    let ancestor_ids = ci.ancestors.iter().map(|a| a.0);
    let ivar_idents = ci.ivars.iter().map(|iv| safe_ident(iv));
    // `hidden_ivars` names `Struct` MEMBERS -- real slots that are not Ruby
    // instance variables (see `ruby_class!`'s `hidden` block). `mro` already
    // made the two lists disjoint, so the macro never has to subtract one from
    // the other, and member order is `Struct.new`'s argument order.
    let hidden_idents = ci.hidden_ivars.iter().map(|iv| safe_ident(iv));

    let methods = ci.methods.iter().map(|entry| {
        let sid = entry.def;
        let scope = compiler.scope(sid);
        let method_ident = safe_ident(&scope.name);
        let needs_block = scope.needs_block_param();
        let sig_params = params::emit_signature_params(&scope.params, needs_block);
        // A shared body already carries this method's frame and `check_ints`,
        // so the forwarding line adds neither. Consuming `self` rather than
        // cloning it makes `new_handle` a pure unsize coercion.
        if let Some(shared_fn) = shared.call(cid, sid) {
            let fwd = params::emit_forward_args(&scope.params, needs_block);
            return quote! {
                def #method_ident(self: std::sync::Arc<Self> #sig_params) {
                    __sh::#shared_fn(zeo_rt::RubyValue::Object(Self::new_handle(self)) #fwd)
                }
            };
        }
        let InstanceMethodBody { preamble, body } = emit_instance_method_body(compiler, cid, sid);
        quote! {
            def #method_ident(self: std::sync::Arc<Self> #sig_params) {
                #preamble
                #body
            }
        }
    });
    let dispatch_entries = ci.methods.iter().map(|entry| {
        let scope = compiler.scope(entry.def);
        // `ci.methods` is the FLATTENED ancestry, so this runs once per
        // (class x visible method) -- and the guard depends on the scope
        // alone, never on `cid`. Uncached, every class that inherited a
        // method re-walked its whole body for the svar question;
        // `analyze::share` records 152 classes behind a single `Prism::Node`
        // method, which is 152 walks for one answer.
        let frame = cached_frame_guard(compiler, entry.def, false);
        let tramp = match compiler.accessor_shape(cid, scope) {
            Some(shape) => {
                let slot = expr::slot_of(compiler, cid, &shape.ivar)
                    .expect("`accessor_shape` only matches a declared slot");
                params::emit_accessor_trampoline(&name_ident, shape, slot, &frame)
            }
            None => params::emit_dynamic_trampoline(
                &name_ident,
                &scope.name,
                &scope.params,
                scope.needs_block_param(),
                &frame,
            ),
        };
        // The dispatch KEY is the method's real Ruby name (`"tag="`), a
        // plain string literal -- NOT `safe_ident(&scope.name)` (the
        // escaped Rust identifier, `tag_set`): `ruby_class!`'s `dispatch`
        // block registers this string directly (`Symbol::intern($dname)`),
        // so using the escaped identifier here would register the method
        // under the wrong runtime name entirely, breaking `send`/a rescued
        // exception's own `.send(:tag=, ...)` for any escaped-name method.
        let dispatch_key = &scope.name;
        quote! { #dispatch_key => #tramp }
    });

    quote! {
        zeo_rt::ruby_class! {
            class #name_ident : #parent_ty {
                id: #id;
                name: #fq_name;
                ancestors: [ #(#ancestor_ids),* ];
                ivars { #(#ivar_idents),* }
                hidden { #(#hidden_idents),* }
                #(#methods)*
                dispatch { #(#dispatch_entries),* }
            }
        }
    }
}
