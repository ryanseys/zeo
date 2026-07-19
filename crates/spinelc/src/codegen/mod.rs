//! Emits Rust source for the analyzed program -- the direct analog of
//! `codegen_program` (`codegen.c:4275`): walks classes/methods/top-level
//! statements, emitting `ruby_class!` macro invocations plus a `fn main()`.
//!
//! Generated Rust is built as a `proc_macro2::TokenStream`, composed via
//! `quote!`, rather than hand-formatted `String`/`format!` text -- each
//! `emit_*` function returns a fragment, spliced into its caller's `quote!`
//! rather than string-interpolated. This is a code-generation *library*
//! choice, not proc-macro infrastructure operating on `spinelc` itself:
//! `ruby_class!` (defined in `spinel-rt`) still only expands when the
//! *generated program* is compiled by the real `cargo build` in a temp
//! project, exactly as before. `codegen_to_string` re-parses the finished
//! `TokenStream` with `syn` and formats it with `prettyplease` -- a free
//! correctness net: a codegen bug that produces invalid Rust now fails right
//! here as a clean `Result::Err`, not later as a confusing `cargo build`
//! failure in a temp dir.

mod call;
mod captures;
mod collections;
mod exceptions;
mod expr;
mod hoisting;
mod ident;
mod loops;
mod params;
mod patterns;
mod stmt;

use quote::{format_ident, quote};

use crate::analyze::Analyzed;
use crate::compiler::{ClassId, Compiler, OBJECT_CLASS};
use crate::types::TyKind;
use ident::safe_ident;
use proc_macro2::TokenStream;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use syn::Lifetime;

#[derive(Clone)]
struct Ctx<'a> {
    compiler: &'a Compiler,
    /// Which `Ruby::Box` the code currently being emitted is DEFINED in --
    /// the AOT analogue of CRuby's `cme->def->box` stamp (a method resolves
    /// names against its DEFINING box, never its caller's). `0` (the main
    /// box) everywhere until Phase 18 lights it up: a method body carries
    /// its `defining_class`'s box, a `BoxScope` body overrides it. Consumed
    /// by `Ctx::resolve_class` (and, from Phase 18, gvar/dispatch emission).
    box_id: u32,
    /// The RECEIVER's concrete class -- i.e. which `impl` block (generated
    /// Rust struct) this method body is being emitted into. Stays fixed
    /// across nested `super` splices (unlike `defining_class` below), since
    /// `self` is always the SAME concrete instance throughout.
    current_class: Option<ClassId>,
    current_method: Option<String>,
    /// Which class/module's HIR body the CURRENTLY-executing method
    /// actually came from -- equal to `current_class` for an ordinary
    /// own-body method, but set to the true source ancestor while emitting
    /// a materialized (inherited or mixed-in) method, or while inlining a
    /// `super` splice (see `compiler::Scope::defining_class`'s docs). Two
    /// distinct roles read this: `super` resolution
    /// (`codegen::call::emit_super_inline`) searches `current_class`'s
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
    /// see `spinel_rt::civars`' docs for the `Sub.reg` => nil case that
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
    /// break, so a loop's own body simply shadows this field for itself, and
    /// `emit_super_inline` preserves the *caller's* labels unchanged (the
    /// inlined parent body is spliced at the call site, so a `break` inside
    /// it must still target whatever loop lexically encloses that call
    /// site, exactly as if the code were written there directly).
    loop_labels: Option<(Lifetime, Lifetime)>,
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
    /// this construct's own body" shape -- but, unlike `loop_labels`,
    /// `emit_super_inline` does NOT carry this over into the inlined parent
    /// body: the override names a specific local by NAME, and the parent
    /// method's own locals are a different Ruby scope that just might
    /// happen to reuse that name for something unrelated (a narrow,
    /// defensive choice -- worst case without it is a missed optimization,
    /// not a wrong answer).
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
    /// The identifier that stands for `self` in THIS position -- ordinarily
    /// the literal `self`, but rebound to a fresh capture-alias identifier
    /// while emitting an escaping block's own body that captured `self`
    /// (`let self = ...;` is illegal Rust -- `self` is only bindable as a
    /// receiver parameter -- so the closure clones into a DIFFERENT name;
    /// see `codegen::call`'s Proc-construction docs). Consulted everywhere
    /// an ivar is read/written (`codegen::expr`'s `IvarRead`/`IvarWrite`)
    /// instead of a hardcoded `self`. `emit_super_inline` carries this over
    /// into the inlined parent body (unlike `for_var_override`): the
    /// splice needs to keep referring to whichever `self` the CALLING
    /// method's body is already using.
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
    /// Whether `self_ident` names a `RubyValue` whose concrete class isn't
    /// statically known, rather than an `Arc<Concrete>`/`self`. True exactly
    /// inside an escaping Proc that captured `self`: such a block's receiver
    /// is a closure PARAMETER (`RProc::with_self`), because `instance_exec`
    /// can run the very same block body under a different one -- so nothing
    /// about the receiver's class is known until the call happens.
    ///
    /// Ivar access consults this (`expr`'s `IvarRead`/`IvarWrite`): a
    /// statically-typed self reads a struct FIELD, a dynamic one goes
    /// through `spinel_rt::ivar_get_dyn`/`ivar_set_dyn`'s name-keyed lookup.
    /// So does implicit-self dispatch (`call`'s `boxed_implicit_self`).
    /// Inline-spliced blocks (`.times` and friends) are NOT affected: they
    /// are not `Proc`s, can't be handed to `instance_exec`, and keep the
    /// static field fast path.
    self_is_dynamic: bool,
}

impl<'a> Ctx<'a> {
    /// Resolves a class/module NAME as seen from the code currently being
    /// emitted -- the one funnel every codegen name-resolution site goes
    /// through (Phase 15.1), keyed by the lexical cref chain (derived from
    /// `defining_class`'s `lexical_parent` links, so there's no separate
    /// context field to thread) and this context's box. See
    /// `Compiler::resolve_class` for the resolution order.
    fn resolve_class(&self, name: &str) -> Option<ClassId> {
        self.compiler
            .resolve_class(name, &self.cref_chain(), self.box_id)
    }

    /// The lexical scope chain enclosing the current code, outermost first
    /// (`resolve_class` walks it back-to-front, i.e. innermost-outward) --
    /// `Compiler::cref_of`'s rule (Phase 15.3), which also honors the
    /// qualified-definition cut (see `ClassInfo::qualified_def`). Empty at
    /// the top level.
    fn cref_chain(&self) -> Vec<ClassId> {
        self.compiler.cref_of(self.defining_class)
    }

    /// A child context for a native loop's own body -- see `loop_labels`'s
    /// docs.
    fn in_loop(&self, redo: Lifetime, outer: Lifetime) -> Ctx<'a> {
        Ctx {
            loop_labels: Some((redo, outer)),
            ..self.clone()
        }
    }

    /// A child context for a `BoxScope` body (Phase 18): everything inside
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
    // Not consumed yet -- wired up together with Proc construction, later in
    // this same phase.
    #[allow(dead_code)]
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
            captured_locals: shadow(self.captured_locals.clone()),
            local_types,
            ..self.clone()
        }
    }
}

/// Wrap a method body that needs a `Signal::Return` catch. `home_push`/
/// `home_pop` bracket the activation so a Proc constructed inside it captures
/// this frame as its non-local-return home (see `spinel_rt::signal`); the pop
/// marks the home dead on EVERY exit (normal or signal), so a later `return`
/// through a Proc whose home has unwound raises `LocalJumpError`. Then the
/// method's own `Signal::Return` folds to a normal value. A method with no
/// escaping block / `begin` catches nothing and pushes no home (a bare-`yield`
/// method must not intercept a `Return` meant for a different frame).
fn wrap_method_return(needs_return_catch: bool, inner: TokenStream) -> TokenStream {
    if needs_return_catch {
        quote! {
            spinel_rt::home_push();
            let __ret = (|| -> Result<spinel_rt::RubyValue, spinel_rt::Signal> { #inner })();
            spinel_rt::home_pop();
            __ret.or_else(|__e| match __e {
                spinel_rt::Signal::Return(__v) => Ok(__v),
                __e => Err(__e),
            })
        }
    } else {
        inner
    }
}

/// The `spinel_rt::register_params` entries for one method's `Params`, in
/// Ruby's canonical `#parameters` order (required, optional, rest, post,
/// keywords, keyword-rest, block). Internal destructure-slot names
/// (`__destr_N`) are emitted anonymous, matching CRuby's nameless `[:req]`
/// for a `|(a, b)|` slot.
fn param_descriptor_entries(params: &crate::hir::Params) -> Vec<TokenStream> {
    use crate::hir::KeywordParam;
    fn entry(kind: &str, name: Option<&str>) -> TokenStream {
        let k = format_ident!("{}", kind);
        match name {
            Some(n) if !n.starts_with("__") => {
                quote! { (spinel_rt::ParamKind::#k, Some(#n.to_string())) }
            }
            _ => quote! { (spinel_rt::ParamKind::#k, None) },
        }
    }
    let mut out = Vec::new();
    for r in &params.required {
        out.push(entry("Req", Some(r)));
    }
    for (o, _) in &params.optional {
        out.push(entry("Opt", Some(o)));
    }
    match &params.rest {
        Some(Some(n)) => out.push(entry("Rest", Some(n))),
        Some(None) => out.push(entry("Rest", None)),
        None => {}
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
    match &params.keyword_rest {
        Some(Some(n)) => out.push(entry("KeyRest", Some(n))),
        Some(None) => out.push(entry("KeyRest", None)),
        None => {}
    }
    match &params.block {
        Some(Some(n)) => out.push(entry("Block", Some(n))),
        Some(None) => out.push(entry("Block", None)),
        None => {}
    }
    out
}

pub fn codegen_to_string(analyzed: &Analyzed) -> Result<String, String> {
    let tokens = codegen(analyzed);
    let file: syn::File = syn::parse2(tokens)
        .map_err(|e| format!("codegen produced invalid Rust (this is a spinelc bug): {e}"))?;
    Ok(format!(
        "// Generated by spinelc. Do not edit by hand.\n\n{}",
        prettyplease::unparse(&file)
    ))
}

fn codegen(analyzed: &Analyzed) -> TokenStream {
    let compiler = &analyzed.compiler;

    // `Object` (index 0, built into `spinel-rt`), every MODULE, and every
    // reserved BUILT-IN placeholder (`Integer`/`Array`/etc. -- see
    // `compiler::BUILTIN_CLASSES`) never get a generated Rust struct/
    // `impl RubyObject`/`ClassRegistry` entry via `ruby_class!` at all -- a
    // module's methods only ever manifest indirectly, MATERIALIZED onto
    // whatever includes/prepends/extends it (see the plan's Part 6 and
    // `ruby_class!`'s docs), and a built-in type's runtime representation
    // already IS a `RubyValue` variant, needing no separate struct (see
    // `builtin_registrations` below for how it still gets a `ClassRegistry`
    // entry so `is_a?`/`kind_of?` resolve correctly against it -- its
    // `methods` table stays empty, though, since built-in methods dispatch
    // via hardcoded codegen paths rather than the dynamic registry, so
    // `respond_to?` against one always reports `false`, a separate,
    // pre-existing-shaped scope-cut, not a regression this introduces).
    // BOOTSTRAP classes (the built-in exceptions) are excluded too: their
    // structs/impls/registration now live once in `spinel-rt`, installed by
    // `ClassRegistry::with_core` (see `main` below). The compiler still keeps
    // their HIR for name resolution, `super` inlining, and materializing user
    // subclasses -- it just no longer EMITS them into every program.
    let classes = compiler
        .classes
        .iter()
        .enumerate()
        .filter(|&(idx, _)| compiler.has_generated_struct(ClassId(idx as u32)))
        .map(|(idx, _)| emit_class(compiler, ClassId(idx as u32)));

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
        .map(|(idx, _)| emit_class_methods(compiler, ClassId(idx as u32)));

    // A REOPENED builtin class (Phase 16.3): one `pub mod __bm_<Name>`
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
        .map(|(idx, _)| emit_builtin_reopen(compiler, ClassId(idx as u32)));

    // In FILE order (matching real Ruby's "a class/module body runs
    // immediately as it's defined"): register the class's dispatch table
    // (skipped for a module, which has none), then run any class-body
    // top-level `@@x = expr` statements -- including a MODULE's own, which
    // still needs to run even though a module never gets a `__register()`
    // call of its own (see `ClassInfo::class_body_stmts`'s docs).
    // Class-body statements (`@@x = expr` / `CONST = expr`) are collected
    // separately from the registry calls: they can be FALLIBLE (`CONST =
    // some_call?`), so they run at the head of `run_main`'s closure (where
    // `?` propagates as a Signal) rather than in plain `fn main()`. Still
    // before every top-level statement, and now after the registry install
    // -- both orderings the previous in-main splice already implied.
    let mut user_class_bodies: Vec<TokenStream> = Vec::new();
    let mut builtin_class_bodies: Vec<TokenStream> = Vec::new();

    let mut registrations: Vec<TokenStream> = Vec::new();
    for (idx, class) in compiler.classes.iter().enumerate() {
        // BOOTSTRAP classes are registered by `ClassRegistry::with_core`, not
        // per-program -- skip their whole registration/metadata block here.
        if idx == 0 || class.is_builtin || class.is_bootstrap {
            continue;
        }
        let register = if class.is_module {
            // A module has no generated struct/`__register`, but it
            // still needs a registry entry (Phase 16.1) so its
            // first-class value answers `name`/`.class`/`ancestors`
            // and `puts M` prints its name. No constructor: `M.new`
            // is a real NoMethodError (see `send_value`'s Class arm).
            let id = idx as u32;
            let fq_name = compiler.fq_name(ClassId(id));
            let ancestor_ids = compiler.class(ClassId(id)).ancestors.iter().map(|a| a.0);
            quote! {
                __registry.register(
                    spinel_rt::ClassId(#id),
                    #fq_name,
                    true,
                    vec![#(spinel_rt::ClassId(#ancestor_ids)),*],
                    None,
                );
            }
        } else if compiler.is_exception_backed(ClassId(idx as u32)) {
            // A user `class MyErr < StandardError` (D3): no generated struct --
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
                spinel_rt::register_exception_subclass(
                    &mut __registry,
                    spinel_rt::ClassId(#id),
                    #fq_name,
                    vec![#(spinel_rt::ClassId(#ancestor_ids)),*],
                );
            }
        } else if compiler.is_value_subclass(ClassId(idx as u32)) {
            // A user `class Stack < Array` (D3): no generated struct -- its
            // instances are the native `ValueSubclass` wrapping an `Array`/
            // `String`/`Hash` payload. Register the runtime entry + the shared
            // `value_subclass_construct`; inherited builtin methods come via the
            // `send_in` payload bridge, the subclass's own `def`s via deltas.
            let id = idx as u32;
            let fq_name = compiler.fq_name(ClassId(id));
            let ancestor_ids = compiler.class(ClassId(id)).ancestors.iter().map(|a| a.0);
            quote! {
                spinel_rt::register_value_subclass(
                    &mut __registry,
                    spinel_rt::ClassId(#id),
                    #fq_name,
                    vec![#(spinel_rt::ClassId(#ancestor_ids)),*],
                );
            }
        } else if compiler.is_immediate_subclass(ClassId(idx as u32)) {
            // A user `class MyInt < Integer` (D3): allowed as a DEFINITION but
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
                    spinel_rt::ClassId(#id),
                    #fq_name,
                    false,
                    vec![#(spinel_rt::ClassId(#ancestor_ids)),*],
                    None,
                );
            }
        } else {
            let ident = ident::class_ident(compiler, ClassId(idx as u32));
            quote! { #ident::__register(&mut __registry); }
        };
        registrations.push(register);
        // Each PRIVATE method materialized onto this class -- its own
        // `private def x`, plus every top-level `def` (a private method of
        // Object, which materialization copies onto every class) -- is
        // recorded so `respond_to?` skips it. Emitted next to the class's
        // registration rather than inside `ruby_class!`, which has no
        // visibility channel of its own. See `ClassRegistry::mark_private`.
        let id = idx as u32;
        for &sid in &compiler.class(ClassId(id)).methods {
            let scope = compiler.scope(sid);
            if matches!(scope.visibility, crate::hir::Visibility::Private) {
                let key = &scope.name;
                registrations.push(quote! {
                    __registry.mark_private(
                        spinel_rt::ClassId(#id),
                        spinel_rt::Symbol::intern(#key),
                    );
                });
            }
        }
        // Each method DEFINED DIRECTLY on this class (not materialized from an
        // ancestor) is recorded so `instance_methods(false)`/`methods(false)`
        // can report own methods only -- the materialized `methods` list above
        // flattens inheritance in. Sorted for stable generated source.
        let mut own: Vec<&String> = compiler
            .class(ClassId(id))
            .own_methods
            .iter()
            .map(|&sid| &compiler.scope(sid).name)
            .collect();
        own.sort();
        for key in own {
            registrations.push(quote! {
                __registry.mark_own(
                    spinel_rt::ClassId(#id),
                    spinel_rt::Symbol::intern(#key),
                );
            });
        }
        // The signature of each own method, baked for `Method#arity`/
        // `#parameters` / `UnboundMethod` reflection (the dispatch tables carry
        // only fn pointers). Emitted in Ruby's canonical `#parameters` order.
        for &sid in &compiler.class(ClassId(id)).own_methods {
            let scope = compiler.scope(sid);
            let name = &scope.name;
            let entries = param_descriptor_entries(&scope.params);
            registrations.push(quote! {
                spinel_rt::register_params(#id, #name, vec![ #(#entries),* ]);
            });
        }
        // Every `undef name` in this class's body is recorded so
        // `respond_to?` stops its ancestor walk here -- dispatch itself
        // needs nothing, since `mro::materialize_methods` already left the
        // name out of this class's table. See `ClassEntry::undefined_methods`.
        // Sorted: a HashSet has no stable order, and generated source should
        // not vary between compiles of the same program.
        let mut undefined: Vec<&String> = compiler.class(ClassId(id)).undefined.iter().collect();
        undefined.sort();
        for key in undefined {
            registrations.push(quote! {
                __registry.mark_undefined(
                    spinel_rt::ClassId(#id),
                    spinel_rt::Symbol::intern(#key),
                );
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
        for &sid in &compiler.class(ClassId(id)).class_methods {
            let scope = compiler.scope(sid);
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
            );
            // Keyed on the real Ruby name, not the mangled Rust ident.
            let key = &scope.name;
            registrations.push(quote! {
                __registry.define_class_method(
                    spinel_rt::ClassId(#id),
                    spinel_rt::Symbol::intern(#key),
                    #tramp,
                );
            });
        }
        user_class_bodies.push(emit_class_body_stmts(compiler, ClassId(idx as u32)));
    }

    // Native-exception reopen/subclass DELTAS (D3): the native default method
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
        // mapping since Phase 17.1: its ancestors are COMPUTED
        // (`[Object, Kernel, BasicObject]`), no longer a hardcoded
        // `vec![Object]` in `main()`.
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
            // A per-box OVERLAY (Phase 18) never registers a class entry of
            // its own -- instances keep the ROOT builtin's identity -- but
            // its methods/body statements below still run (registered on
            // the root's entry, keyed by the box).
            let is_overlay = class.builtin_overlay.is_some();
            // A REOPENED builtin (Phase 16.3): each of its methods (own or
            // module-included, all already materialized) registers as a
            // value method so `send_value` dispatches it FIRST -- see
            // `ValueMethodFn`'s docs for the precedence contract. Its
            // `@@cvar = .../CONST = ...` body statements run here too --
            // slightly earlier than their file position (builtins register
            // ahead of user classes), a documented approximation that only
            // matters if a builtin's class body reads a user class.
            let value_defs = class.methods.iter().map(|&sid| {
                let scope = compiler.scope(sid);
                let mod_ident = ident::class_ident(compiler, ClassId(id));
                let method_ident = safe_ident(&scope.name);
                let fn_path = quote! { #mod_ident::#method_ident };
                let tramp = params::emit_value_trampoline(
                    &fn_path,
                    &scope.name,
                    &scope.params,
                    scope.needs_block_param(),
                    params::RecvMode::Pass,
                );
                // The dispatch KEY is the real Ruby name, not the escaped
                // Rust ident -- same reasoning as `emit_class`'s
                // `dispatch_key`.
                let key = &scope.name;
                let ci = compiler.class(ClassId(id));
                let box_id = ci.box_id;
                // A per-box OVERLAY's methods register on the ROOT
                // builtin's entry, keyed by the overlay's box (Phase 18).
                let target = ci.builtin_overlay.map_or(id, |root| root.0);
                // A PRIVATE `def` (every top-level def, and an explicit
                // `private def x`) is recorded so `respond_to?` skips it --
                // see `ClassRegistry::mark_private`.
                let mark_private = matches!(scope.visibility, crate::hir::Visibility::Private)
                    .then(|| {
                        quote! {
                            __registry.mark_private(
                                spinel_rt::ClassId(#target),
                                spinel_rt::Symbol::intern(#key),
                            );
                        }
                    });
                quote! {
                    __registry.define_value_method(
                        spinel_rt::ClassId(#target),
                        #box_id,
                        spinel_rt::Symbol::intern(#key),
                        #tramp,
                    );
                    #mark_private
                }
            });
            builtin_class_bodies.push(emit_class_body_stmts(compiler, ClassId(id)));
            // Always-on builtins with their DEFAULT ancestors are registered
            // once by `spinel_rt::register_builtins` -- so emit a base register
            // here only for a require-gated extension (per-program, and the loop
            // already feature-gates it) or a builtin whose ancestors a reopen
            // actually changed (`class Array; include M; end`). The latter is an
            // OVERRIDE: it lands after `register_builtins` in `main` and replaces
            // the default entry. This keeps the common program free of the ~540
            // identical builtin registrations while preserving full reopen parity.
            let is_ext = class.feature_gate.is_some();
            let ancestors_default =
                class.ancestors == spinel_abi::declared_ancestors(ClassId(id));
            let register = (!is_overlay && (is_ext || !ancestors_default)).then(|| {
                quote! {
                    __registry.register(
                        spinel_rt::ClassId(#id),
                        #name,
                        #is_module,
                        vec![#(spinel_rt::ClassId(#ancestor_ids)),*],
                        None,
                    );
                }
            });
            builtin_registrations.push(quote! {
                #register
                #(#value_defs)*
            });
    }

    let main_label_counter = Cell::new(0u32);
    // Top-level implicit-self calls dispatch on the global `main_object()`
    // (no capture needed), so no `self_class` here.
    let main_captures = captures::collect_escaping_captures(
        compiler,
        &analyzed.main_statements,
        &crate::hir::Params::default(),
        None,
    );
    let cx = Ctx {
        compiler,
        box_id: 0,
        current_class: None,
        defining_class: None,
        // Top-level `self` is `main`, an ordinary Object -- not a class.
        class_self: None,
        current_method: None,
        local_types: std::borrow::Cow::Borrowed(&analyzed.main_local_types),
        label_counter: &main_label_counter,
        loop_labels: None,
        for_var_override: None,
        captured_locals: std::borrow::Cow::Borrowed(&main_captures.locals),
        self_ident: format_ident!("self"),
        in_real_proc: false,
        self_is_dynamic: false,
    };
    let main_body = hoisting::emit_hoisted_body(&cx, &analyzed.main_statements, true);

    // The exception factory that generated `main()` used to install (one arm
    // per runtime-raisable exception class, each an `emit_boxed_new`) is gone:
    // `spinel-rt` now constructs exceptions by NAME from the registered classes
    // (`ClassRegistry::construct_exception`), so this ~1,130-line block no
    // longer bloats every binary. The prelude classes still register their
    // `ConstructorFn` via `ruby_class!`'s `__register`, which is what the
    // runtime construction path uses.

    quote! {
        #(#classes)*
        #(#class_method_containers)*
        #(#builtin_reopens)*
        #(#exc_containers)*

        fn main() {
            // A registry pre-populated with the CORE world -- the always-on
            // builtin classes/modules AND the built-in exception hierarchy,
            // installed once from `spinel-rt` instead of the ~540 identical
            // `register(...)` calls plus ~6,600 lines of `ruby_class!` blocks
            // every program used to emit. Their ids are the ones `spinel-abi`
            // reserves and the compiler asserts it assigned identically (see
            // `analyze`).
            let mut __registry = spinel_rt::ClassRegistry::with_core();
            // Require-gated exts and reopen-modified builtins register on top
            // (the latter as an override that replaces the default entry),
            // then the program's own user classes.
            #(#builtin_registrations)*
            #(#registrations)*
            spinel_rt::install_class_registry(__registry);
            // Seed the CORE constants (`Float::INFINITY`, `Encoding::UTF_8`,
            // `Regexp::IGNORECASE`, `ARGV`, `STDOUT`/`$stdout`, `ENV`,
            // `Process::CLOCK_*`) now that the registry is in place -- their
            // owners resolved at compile time; only the values need installing.
            spinel_rt::install_core_constants();
            // The runtime raises real, catchable exceptions (NoMethodError,
            // ArgumentError, TypeError, StopIteration, ...) by constructing
            // them itself from the registered classes -- see
            // `ClassRegistry::construct_exception`. No per-program factory is
            // installed anymore: it was ~1,130 lines of identical machinery in
            // every binary (the single largest slice after the prelude classes).

            // The whole top level runs as `may`'s first coroutine (Phase
            // 13.4) -- see `spinel_rt::run_main`'s docs for the worker-count
            // GVL model and why registration must complete first. The
            // closure is `move + Send + 'static` trivially: top-level
            // statements are self-contained (their hoisted locals are
            // declared inside the body itself) and every value is Send+Sync
            // (Part 9).
            let __result: Result<spinel_rt::RubyValue, spinel_rt::Signal> =
                spinel_rt::run_main(move || {
                    // Class-body statements (`@@x = expr` / `CONST = expr`)
                    // run first, inside the fallible closure (they may
                    // `?`), builtins before user classes -- the same
                    // "before every top-level statement" order the old
                    // in-`main()` splice had.
                    #(#builtin_class_bodies)*
                    #(#user_class_bodies)*
                    #main_body
                });
            // `at_exit` handlers (reverse order), before uncaught-exception
            // reporting -- CRuby runs them on both the normal and the
            // uncaught path. (`Kernel#exit` runs them itself.)
            spinel_rt::run_at_exit();
            if let Err(__signal) = __result {
                match __signal {
                    // An uncaught `raise` gets a real, Ruby-flavored
                    // message -- calling the exception's own `message`
                    // dynamically (Path 2), exactly as real Ruby's default
                    // top-level handler does, rather than the generic
                    // catch-all below. No runtime class-name table exists
                    // yet (a narrow, deliberate simplification -- real
                    // Ruby's own `"msg (ClassName)"` form needs one), so
                    // only the message is shown until that lands.
                    spinel_rt::Signal::Raise(__exc) => {
                        let __msg = spinel_rt::send(
                            &__exc.as_object_unchecked(),
                            spinel_rt::Symbol::intern("message"),
                            &[],
                            None,
                        )
                        .map(|v| v.to_display_string())
                        .unwrap_or_default();
                        eprintln!("uncaught exception: {}", __msg);
                        std::process::exit(1);
                    }
                    __other => {
                        eprintln!("uncaught signal escaped the top level: {:?}", __other);
                        std::process::exit(1);
                    }
                }
            }
        }
    }
}

/// Every statement written directly in a class/module body -- cvar/const/ivar
/// writes AND general code (a method call, an `each` loop, a runtime
/// `define_method`) -- run once at class-definition time, in file order, with
/// `self` = the class object (`class_self: Some(cid)`). Emitted inside
/// `run_main`'s fallible closure (see the call site), so a fallible statement
/// propagates its `Signal` through `?` like any method-body statement (#97
/// F2a). Runs right after this class/module's own dispatch-table registration.
fn emit_class_body_stmts(compiler: &Compiler, cid: ClassId) -> TokenStream {
    let stmts = &compiler.class(cid).class_body_stmts;
    if stmts.is_empty() {
        return quote! {};
    }
    let label_counter = Cell::new(0u32);
    let no_captures = HashSet::new();
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
        local_types: std::borrow::Cow::Borrowed(&no_locals),
        label_counter: &label_counter,
        loop_labels: None,
        for_var_override: None,
        captured_locals: std::borrow::Cow::Borrowed(&no_captures),
        self_ident: format_ident!("self"),
        in_real_proc: false,
        self_is_dynamic: false,
    };
    // Hoist the class body's own locals and emit its statements as a scoped
    // block that evaluates to `Result` -- `?` propagates a raised Signal into
    // the enclosing `run_main` closure, and the block scopes the locals so they
    // don't leak into `main_body` (a class body is its own scope in Ruby). The
    // `wrap_ok` tail is discarded by `?;` -- class-body statements run for
    // effect; `main_body` supplies the program's tail.
    let body = hoisting::emit_hoisted_body(&cx, stmts, true);
    quote! { { #body }?; }
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
/// Full `Params` support since P1 (same signature/prologue machinery as
/// instance methods, receiverless). **Remaining scope-cut**: a class
/// method's own body may not reference `self`/`@ivar` -- there's no
/// concrete instance for `self` to mean here (a class-level ivar /
/// `class << self` state store is the plan's G5(d), not attempted yet).
fn emit_class_methods(compiler: &Compiler, cid: ClassId) -> TokenStream {
    let ci = compiler.class(cid);
    let name_ident = ident::class_ident(compiler, cid);
    let fns = ci.class_methods.iter().map(|&sid| emit_class_method_fn(compiler, sid));
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
        // Found by Phase 14.2's cross-package test (`Greet.hi` calling
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

fn emit_class_method_fn(compiler: &Compiler, sid: crate::compiler::ScopeId) -> TokenStream {
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
    let no_captures = captures::collect_escaping_captures(compiler, &scope.body, &scope.params, None);
    let cx = Ctx {
        compiler,
        box_id: compiler.class(scope.defining_class).box_id,
        // No concrete receiver exists for a class method (no `self:
        // Arc<Self>`) -- but `defining_class` (which class/module this body
        // was LEXICALLY written in) still needs to be real, for `@@cvar`
        // ownership lookup (`codegen::expr::cvar_owner_id`).
        current_class: None,
        defining_class: Some(scope.defining_class),
        // `scope.class` (the OWNER -- which class this body is emitted
        // into), deliberately NOT `defining_class` (where it was written).
        // The two differ exactly when a class method is inherited, and
        // that is precisely the case class-ivar storage must tell apart:
        // `Sub.reg` reads Sub's slot even though the body came from Base.
        // See `Ctx::class_self`'s docs.
        class_self: scope.class,
        current_method: Some(scope.name.clone()),
        local_types: std::borrow::Cow::Borrowed(&scope.local_types),
        label_counter: &label_counter,
        loop_labels: None,
        for_var_override: None,
        captured_locals: std::borrow::Cow::Borrowed(&no_captures.locals),
        self_ident: format_ident!("self"),
        in_real_proc: false,
        self_is_dynamic: false,
    };
    let prologue = params::emit_prologue(&cx, &scope.params);
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
        || captures::body_contains_escaping_block(compiler, &scope.body);
    let body_tokens = wrap_method_return(needs_return_catch, body);
    quote! {
        #[allow(unused_variables)]
        pub fn #method_ident(#sig_params) -> Result<spinel_rt::RubyValue, spinel_rt::Signal> {
            #body_tokens
        }
    }
}

/// The generated container for a REOPENED builtin class (Phase 16.3) -- a
/// `pub mod __bm_<Name>` (see `ident::class_ident`'s builtin mangling for
/// why not a bare `pub mod String`) holding the reopen's instance methods
/// as FREE FUNCTIONS (`__self: RubyValue` first parameter -- a builtin has
/// no struct for a `self: Arc<Self>` receiver) and its `def self.x` class
/// methods (the same `emit_class_method_fn` shape a module container gets).
/// Both kinds share one module, so one name can't be both -- a clean panic
/// rather than a confusing generated-`rustc` duplicate-definition error.
fn emit_builtin_reopen(compiler: &Compiler, cid: ClassId) -> TokenStream {
    let ci = compiler.class(cid);
    for &sid in &ci.methods {
        let n = &compiler.scope(sid).name;
        if ci
            .class_methods
            .iter()
            .any(|&cs| compiler.scope(cs).name == *n)
        {
            panic!(
                "the built-in class `{}` defines both an instance method and a class method named `{n}` -- not supported yet (spike scope: they share one generated container)",
                ci.name
            );
        }
    }
    let mod_ident = ident::class_ident(compiler, cid);
    let instance_fns = ci
        .methods
        .iter()
        .map(|&sid| emit_builtin_method_fn(compiler, cid, sid));
    let class_fns = ci
        .class_methods
        .iter()
        .map(|&sid| emit_class_method_fn(compiler, sid));
    // `use super::*;` for the same reason a module's class-method container
    // needs it (see `emit_class_methods`): a `pub mod` is a real child
    // module, and these bodies reference sibling top-level items.
    quote! {
        #[allow(non_snake_case)]
        pub mod #mod_ident { #[allow(unused_imports)] use super::*; #(#instance_fns)* #(#class_fns)* }
    }
}

/// The native-exception reopen/subclass DELTA (D3): for an exception-backed
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
        .copied()
        .filter(|&sid| {
            let scope = compiler.scope(sid);
            !scope.native_default && compiler.is_native_backed(scope.defining_class)
        })
        .collect();
    if deltas.is_empty() {
        return None;
    }
    let mod_ident = format_ident!("__exc_{}", cid.0);
    let fns = deltas.iter().map(|&sid| emit_builtin_method_fn(compiler, cid, sid));
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
            let tramp =
                params::emit_exc_trampoline(&fn_path, name, &scope.params, scope.needs_block_param());
            quote! {
                __registry.define_method(
                    spinel_rt::ClassId(#id),
                    spinel_rt::Symbol::intern(#name),
                    #tramp,
                );
            }
        })
        .collect();
    Some((container, regs))
}

/// One reopened-builtin INSTANCE method (Phase 16.3) -- the free-function
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
    let scope = compiler.scope(sid);
    let method_ident = safe_ident(&scope.name);
    let needs_block = scope.needs_block_param();
    let sig_params = params::emit_signature_params(&scope.params, needs_block);
    let label_counter = Cell::new(0u32);
    let method_captures = captures::collect_escaping_captures(compiler, &scope.body, &scope.params, Some(cid));
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
        local_types: std::borrow::Cow::Borrowed(&scope.local_types),
        label_counter: &label_counter,
        loop_labels: None,
        for_var_override: None,
        captured_locals: std::borrow::Cow::Borrowed(&method_captures.locals),
        self_ident: format_ident!("__self"),
        in_real_proc: false,
        // `__self` here is the `RubyValue` receiver parameter, not an
        // `Arc<Concrete>` -- a reopened builtin has no generated struct to
        // take ivar fields from, and for an `Object` reopen (which is where
        // TOP-LEVEL `def`s live) the receiver is the `main` object, whose
        // ivars are name-keyed. This is what used to be rejected as
        // "instance variable `@c` in a top-level method (or `Object` reopen)
        // isn't supported yet (the `main` object has no ivar storage)"; it
        // has storage now (`dispatch::Object`).
        self_is_dynamic: true,
    };
    let prologue = params::emit_prologue(&cx, &scope.params);
    let body = hoisting::emit_hoisted_body_with_extra_roots(
        &cx,
        &scope.body,
        &scope.params.default_ids(),
        &scope.params.bound_names(),
        true,
    );
    // Same needs-a-`Signal::Return`-catch rule as `emit_class`'s methods --
    // see the long comment there.
    let needs_return_catch = captures::body_contains_escaping_block(compiler, &scope.body)
        || captures::body_contains_begin(compiler, &scope.body);
    let body_tokens = wrap_method_return(needs_return_catch, quote! { #prologue #body });
    // `allow(unused_variables)`: a reopen method that never references
    // `self` leaves `__self` unread -- unlike a real `self` receiver
    // parameter, which rustc never warns about.
    quote! {
        #[allow(unused_variables)]
        pub fn #method_ident(__self: spinel_rt::RubyValue #sig_params) -> Result<spinel_rt::RubyValue, spinel_rt::Signal> {
            #body_tokens
        }
    }
}

fn emit_class(compiler: &Compiler, cid: ClassId) -> TokenStream {
    let ci = compiler.class(cid);
    let name_ident = ident::class_ident(compiler, cid);
    // The registry's Ruby-visible name (Phase 16.1): fully qualified, so
    // `puts Store::Item` and NoMethodError messages print the real path.
    let fq_name = compiler.fq_name(cid);
    let parent = ci.parent.unwrap_or(OBJECT_CLASS);
    // A BUILTIN parent (`< Struct`, Phase 17.1-H) has no generated Rust
    // struct -- structurally the class sits on `Object`; the SEMANTIC
    // chain (ancestors, is_a?, rescue) carries the real parent.
    let parent_ty = if parent == OBJECT_CLASS || compiler.class(parent).is_builtin {
        quote! { spinel_rt::Object }
    } else {
        let parent_ident = ident::class_ident(compiler, parent);
        quote! { #parent_ident }
    };
    let id = cid.0;
    let ancestor_ids = ci.ancestors.iter().map(|a| a.0);
    let ivar_idents = ci.ivars.iter().map(|iv| safe_ident(iv));

    let methods = ci.methods.iter().map(|&sid| {
        let scope = compiler.scope(sid);
        let method_ident = safe_ident(&scope.name);
        let needs_block = scope.needs_block_param();
        let sig_params = params::emit_signature_params(&scope.params, needs_block);
        let method_label_counter = Cell::new(0u32);
        let method_captures = captures::collect_escaping_captures(compiler, &scope.body, &scope.params, Some(cid));
        let method_cx = Ctx {
            compiler,
            box_id: compiler.class(scope.defining_class).box_id,
            current_class: Some(cid),
            defining_class: Some(scope.defining_class),
            // An instance method -- see the matching note in `emit_method_fn`.
            class_self: None,
            current_method: Some(scope.name.clone()),
            local_types: std::borrow::Cow::Borrowed(&scope.local_types),
            label_counter: &method_label_counter,
            loop_labels: None,
            for_var_override: None,
            captured_locals: std::borrow::Cow::Borrowed(&method_captures.locals),
            self_ident: format_ident!("self"),
            in_real_proc: false,
            self_is_dynamic: false,
        };
        let prologue = params::emit_prologue(&method_cx, &scope.params);
        let body = hoisting::emit_hoisted_body_with_extra_roots(
            &method_cx,
            &scope.body,
            &scope.params.default_ids(),
            &scope.params.bound_names(),
            true,
        );
        // The `Signal::Return` catch is needed ONLY when this method's OWN
        // body lexically contains an escaping block OR a `begin`/`rescue`
        // construct -- confirmed the hard way NOT to be "wrap every method
        // unconditionally" (a simpler design tried first): a method with
        // neither (e.g. one that just does `yield` to whatever block it's
        // handed) must NOT catch `Signal::Return` in transit, or it would
        // incorrectly intercept a `return` meant for a DIFFERENT method --
        // wherever the block it's currently invoking was actually written --
        // turning "return from the caller" into "this method returns
        // normally instead". See `codegen::captures::body_contains_escaping_block`'s
        // docs, and `codegen::exceptions`'s module docs for why `begin`/
        // `rescue` ALSO needs this (it introduces its own closure boundary a
        // literal `return` can't cross either).
        let needs_return_catch = captures::body_contains_escaping_block(compiler, &scope.body)
            || captures::body_contains_begin(compiler, &scope.body);
        let body_tokens = wrap_method_return(needs_return_catch, quote! { #prologue #body });
        quote! {
            def #method_ident(self: std::sync::Arc<Self> #sig_params) {
                #body_tokens
            }
        }
    });

    let dispatch_entries = ci.methods.iter().map(|&sid| {
        let scope = compiler.scope(sid);
        let tramp = params::emit_dynamic_trampoline(
            &name_ident,
            &scope.name,
            &scope.params,
            scope.needs_block_param(),
        );
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
        spinel_rt::ruby_class! {
            class #name_ident : #parent_ty {
                id: #id;
                name: #fq_name;
                ancestors: [ #(#ancestor_ids),* ];
                ivars { #(#ivar_idents),* }
                #(#methods)*
                dispatch { #(#dispatch_entries),* }
            }
        }
    }
}
