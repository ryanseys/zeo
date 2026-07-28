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
mod collections;
mod constfold;
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
            .resolve_class(name, &self.cref_chain(), self.box_id)
    }

    /// The lexical scope chain enclosing the current code, outermost first
    /// (`resolve_class` walks it back-to-front, i.e. innermost-outward) --
    /// `Compiler::cref_of`'s rule, which also honors the
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
            block_depth: self.block_depth + 1,
            ..self.clone()
        }
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
            zeo_rt::home_pop();
            __ret.or_else(|__e| match __e {
                zeo_rt::Signal::Return(__v) => Ok(__v),
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
) -> Option<(String, u32)> {
    let span = compiler.hir.span(node)?;
    let file = compiler.hir.files.get(span.file.0 as usize)?;
    let upto = (span.start as usize).min(file.source.len());
    Some((file.name.clone(), file.line_at(upto as u32)))
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
    let label = format!(
        "{}{sep}{}",
        compiler.fq_name(scope.defining_class),
        scope.name
    );
    quote! { let __frame = zeo_rt::FrameGuard::push(#file, #label, #line); }
}

/// The frame label of the scope ENCLOSING the current emission position --
/// what a block nested here is labeled under (`block in <this>`): a
/// method (`Class#m` / `Class.m`), a class body (`<class:Foo>`), or
/// `<main>`. Mirrors CRuby's lexical block-frame naming.
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
        let kind = if cx.compiler.class(c).is_module {
            "module"
        } else {
            "class"
        };
        return format!("<{kind}:{}>", cx.compiler.leaf_name(c));
    }
    "<main>".to_string()
}

/// The `zeo_rt::MethodMeta` registration for one method scope: what Ruby can
/// ask back about a `def` that its fn pointer can't answer -- the signature
/// (`#arity`/`#parameters`) and the `def` keyword's own line
/// (`#source_location`, and the tail of `#inspect`). One helper for all three
/// emission sites: a user class's own methods, its `def self.x` methods, and
/// the methods a reopened builtin gains.
///
/// A fact the method doesn't have is left off the chain rather than emitted
/// empty, so the generated registration stays as small as the `def` is.
fn method_meta_registration(
    compiler: &Compiler,
    class: ClassId,
    scope: &crate::compiler::Scope,
    class_method: bool,
) -> TokenStream {
    let id = class.0;
    let name = &scope.name;
    let ctor = format_ident!("{}", if class_method { "singleton" } else { "instance" });
    let entries = param_descriptor_entries(&scope.params);
    let params = (!entries.is_empty()).then(|| quote! { .with_params(vec![ #(#entries),* ]) });
    let defined_at = scope
        .def_node
        .and_then(|n| source_location(compiler, n))
        .map(|(file, line)| quote! { .defined_at(#file, #line) });
    let aliased_from = scope
        .alias_of
        .as_ref()
        .map(|original| quote! { .aliased_from(#original) });
    quote! {
        zeo_rt::MethodMeta::#ctor(#id, #name) #params #defined_at #aliased_from .register();
    }
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
                quote! { (zeo_rt::ParamKind::#k, Some(#n.to_string())) }
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

thread_local! {
    static UNSUPPORTED: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Records a construct codegen can't emit, keeping the FIRST message so the
/// report names the earliest failure rather than the deepest. Emission runs to
/// completion; `codegen_to_string` turns the record into a `CompileError`.
///
/// This exists because a `panic!` here unwinds straight through the test
/// harness, which makes an unsupported construct impossible to check in as an
/// XFAIL repro. Genuine compiler-invariant violations still panic.
pub(crate) fn record_unsupported(message: impl Into<String>) {
    UNSUPPORTED.with_borrow_mut(|slot| {
        slot.get_or_insert_with(|| message.into());
    });
}

/// [`record_unsupported`] plus a `nil` stand-in for expression and statement
/// position. The stand-in is never compiled -- `codegen_to_string` fails before
/// the token stream is parsed.
pub(crate) fn unsupported(message: impl Into<String>) -> TokenStream {
    record_unsupported(message);
    quote! { zeo_rt::RubyValue::Nil }
}

fn take_unsupported() -> Option<String> {
    UNSUPPORTED.with_borrow_mut(|slot| slot.take())
}

/// The assembled program as tokens, with the unsupported-construct record
/// turned into a clean `CompileError` -- the shared front half of both
/// renderers below.
fn codegen_to_tokens(analyzed: &Analyzed) -> Result<TokenStream, crate::diagnostics::CompileError> {
    take_unsupported();
    let tokens = codegen(analyzed);
    if let Some(message) = take_unsupported() {
        return Err(crate::diagnostics::CompileError::codegen(message));
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
    let out = format!("// Generated by zeo. Do not edit by hand.\n\n{tokens}");
    if crate::timings_enabled() {
        eprintln!(
            "zeo-timings: codegen_tokens={tokens_ms}ms emit={}ms",
            t_emit.elapsed().as_millis(),
        );
    }
    Ok(out)
}

/// The HUMAN renderer behind `-S`: the historical `syn` round-trip (a free
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
    let compiler = &analyzed.compiler;

    // `Object` (index 0, built into `zeo-rt`), every MODULE, and every
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
    // structs/impls/registration now live once in `zeo-rt`, installed by
    // `ClassRegistry::with_core` (see `main` below). The compiler still keeps
    // their HIR for name resolution, `super` inlining, and materializing user
    // subclasses -- it just doesn't EMIT them into every program.
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
        .map(|(idx, _)| emit_builtin_reopen(compiler, ClassId(idx as u32)));

    // Class-body statements run at their DOCUMENT position: a site whose
    // `ClassDef` marker is reachable from `main_statements` (directly, via
    // a `BoxScope`, or nested inside another reachable site) emits inline
    // there (`codegen::stmt`'s `ClassDef` arm) -- real Ruby's "a class body
    // runs where it appears, re-running per reopen". Everything else --
    // the prelude's bootstrap bodies and the synthetic `None`-marker
    // registrations -- keeps the hoisted splice at the head of
    // `run_main`'s fallible closure (where `?` propagates as a Signal), so
    // no recorded statement can ever be silently dropped.
    let inline_markers = inline_class_markers(compiler, &analyzed.main_statements);
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
            .map(|s| emit_class_body_site(compiler, s))
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
        // Each PRIVATE method materialized onto this class -- its own
        // `private def x`, plus every top-level `def` (a private method of
        // Object, which materialization copies onto every class) -- is
        // recorded so `respond_to?` skips it. Emitted next to the class's
        // registration rather than inside `ruby_class!`, which has no
        // visibility channel of its own. See `ClassRegistry::mark_private`.
        let id = idx as u32;
        for &sid in &compiler.class(ClassId(id)).methods {
            let scope = compiler.scope(sid);
            let key = &scope.name;
            match scope.visibility {
                crate::hir::Visibility::Private => registrations.push(quote! {
                    __registry.mark_private(
                        zeo_rt::ClassId(#id),
                        zeo_rt::Symbol::intern(#key),
                    );
                }),
                // A protected method is recorded so the `protected_*` reflection
                // and `protected_method_defined?` can report it (and `public_*`
                // exclude it). See `ClassRegistry::mark_protected`.
                crate::hir::Visibility::Protected => registrations.push(quote! {
                    __registry.mark_protected(
                        zeo_rt::ClassId(#id),
                        zeo_rt::Symbol::intern(#key),
                    );
                }),
                crate::hir::Visibility::Public => {}
            }
        }
        // A `private`/`public`/`protected :m` re-declaring an INHERITED method's
        // visibility overrides the materialized stamp above -- emitted after the
        // loop so the override wins (a HashSet insert/remove). `mark_public`
        // clears any private/protected mark, promoting the method.
        for (name, vis) in &compiler.class(ClassId(id)).visibility_overrides {
            let mark = match vis {
                crate::hir::Visibility::Private => quote! {
                    __registry.mark_private(zeo_rt::ClassId(#id), zeo_rt::Symbol::intern(#name));
                },
                crate::hir::Visibility::Protected => quote! {
                    __registry.mark_protected(zeo_rt::ClassId(#id), zeo_rt::Symbol::intern(#name));
                },
                crate::hir::Visibility::Public => quote! {
                    __registry.mark_public(zeo_rt::ClassId(#id), zeo_rt::Symbol::intern(#name));
                },
            };
            registrations.push(mark);
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
                    zeo_rt::ClassId(#id),
                    zeo_rt::Symbol::intern(#key),
                );
            });
        }
        // What each own method's fn pointer can't answer: its signature and
        // its `def` line, baked for `Method`/`UnboundMethod` reflection.
        for &sid in &compiler.class(ClassId(id)).own_methods {
            let scope = compiler.scope(sid);
            registrations.push(method_meta_registration(compiler, ClassId(id), scope, false));
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
                &scope_frame_guard(compiler, scope, true),
            );
            // Keyed on the real Ruby name, not the mangled Rust ident.
            let key = &scope.name;
            registrations.push(quote! {
                __registry.define_class_method(
                    zeo_rt::ClassId(#id),
                    zeo_rt::Symbol::intern(#key),
                    #tramp,
                );
            });
        }
        // A class method's reflection keys on the SINGLETON table, so
        // `Api.method(:fetch)` and `Api.new.method(:fetch)` can't collide --
        // and on the class that WROTE the `def self.x`, not on every
        // descendant materialization gave a copy to, so `Cache.method(:open)`
        // still reports `Store` as its owner.
        for &sid in &compiler.class(ClassId(id)).own_class_methods {
            let scope = compiler.scope(sid);
            let key = &scope.name;
            registrations.push(quote! {
                __registry.mark_own_class_method(
                    zeo_rt::ClassId(#id),
                    zeo_rt::Symbol::intern(#key),
                );
            });
            registrations.push(method_meta_registration(compiler, ClassId(id), scope, true));
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
            .filter(|(_, sid)| !ci.class_methods.contains(sid))
            .copied()
            .collect();
        // One container per (class, module): the same NAME can be shadowed
        // in several sibling modules, and each copy is a distinct fn.
        let mut by_module: Vec<(ClassId, Vec<crate::compiler::ScopeId>)> = Vec::new();
        for &(m, sid) in &shadowed {
            match by_module.iter_mut().find(|(bm, _)| *bm == m) {
                Some((_, sids)) => sids.push(sid),
                None => by_module.push((m, vec![sid])),
            }
        }
        for (m, sids) in by_module {
            let flat = ci.name.replace("::", "_");
            let container = format_ident!("__sst_{}_{}_{}", idx, m.0, flat);
            let fns = sids.iter().map(|&sid| emit_class_method_fn(compiler, sid));
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
            .filter(|(_, sid)| ci.class_methods.contains(sid))
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
    let mut super_reachable: std::collections::HashSet<&str> = compiler
        .scopes
        .iter()
        .filter(|scope| {
            scope
                .body
                .iter()
                .any(|&n| crate::analyze::scan_contains_super(&compiler.hir, n))
        })
        .map(|scope| scope.name.as_str())
        .collect();
    // `Method#super_method` re-seats a bound Method onto the ancestor that
    // defines the name NEXT, and calling that Method must run the ancestor's
    // body -- which needs exactly the receiver-generic bridge `super` needs.
    // Which name and which ancestor it lands on is a runtime choice, so a
    // program that reflects this way marks every method name reachable. Same
    // over-approximation policy as the runtime-`super` shapes below: a
    // spurious bridge is dead code, a missing one is a wrong NoMethodError.
    if compiler.hir.all_nodes().iter().any(|node| {
        matches!(node, crate::hir::HirNode::Call { name, .. } if name == "super_method")
    }) {
        super_reachable.extend(compiler.scopes.iter().map(|scope| scope.name.as_str()));
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
    for node in compiler.hir.all_nodes() {
        match node {
            crate::hir::HirNode::DefMethod { name, body, .. }
                if crate::analyze::scan_contains_super_body(&compiler.hir, body) =>
            {
                super_reachable.insert(name.as_str());
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
                            super_reachable.insert(n.as_str());
                        }
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
            if !super_reachable.contains(name.as_str()) {
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
        let flat = class.name.replace("::", "_");
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
                &scope_frame_guard(compiler, scope, false),
            );
            // The dispatch KEY is the real Ruby name, not the escaped
            // Rust ident -- same reasoning as `emit_class`'s
            // `dispatch_key`.
            let key = &scope.name;
            let ci = compiler.class(ClassId(id));
            let box_id = ci.box_id;
            // A per-box OVERLAY's methods register on the ROOT
            // builtin's entry, keyed by the overlay's box.
            let target = ci.builtin_overlay.map_or(id, |root| root.0);
            // A PRIVATE `def` (every top-level def, and an explicit
            // `private def x`) is recorded so `respond_to?` skips it; a
            // PROTECTED one so the `protected_*` reflection reports it. See
            // `ClassRegistry::mark_private`/`mark_protected`.
            let mark_vis = match scope.visibility {
                crate::hir::Visibility::Private => Some(quote! {
                    __registry.mark_private(
                        zeo_rt::ClassId(#target),
                        zeo_rt::Symbol::intern(#key),
                    );
                }),
                crate::hir::Visibility::Protected => Some(quote! {
                    __registry.mark_protected(
                        zeo_rt::ClassId(#target),
                        zeo_rt::Symbol::intern(#key),
                    );
                }),
                crate::hir::Visibility::Public => None,
            };
            // Bake this method's reflection facts -- the `own_methods` loop
            // above only covers user CLASSES, not a reopened builtin (Object,
            // which every top-level `def` materializes onto).
            let meta = method_meta_registration(compiler, ClassId(target), scope, false);
            quote! {
                __registry.define_value_method(
                    zeo_rt::ClassId(#target),
                    #box_id,
                    zeo_rt::Symbol::intern(#key),
                    #tramp,
                );
                #meta
                #mark_vis
            }
        });
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
        builtin_registrations.push(quote! {
            #register
            #(#value_defs)*
            #(#alias_rows)*
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
        runtime_super_params: None,
        block_depth: 0,
        has_blk_binding: false,
    };
    let main_body = hoisting::emit_hoisted_body(&cx, &analyzed.main_statements, true);
    // The top level's own backtrace frame -- CRuby's `<main>` (its file is
    // the main script; statement emission stamps the line as it goes).
    let main_frame = match compiler.hir.files.first() {
        Some(f) => {
            let file = &f.name;
            quote! { let __frame = zeo_rt::FrameGuard::push(#file, "<main>", 0); }
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

    // User-module method bridges (see `emit_user_module_bridges`): their value-
    // method registrations run LAST, after every module's own
    // `__registry.register` above has created the entry they attach to.
    let (um_containers, um_regs) = emit_user_module_bridges(compiler);
    registrations.extend(um_regs);

    // Exceptions are constructed at runtime by NAME from the registered
    // classes (`ClassRegistry::construct_exception`) -- no per-program
    // factory. The prelude classes register their `ConstructorFn` via
    // `ruby_class!`'s `__register`, which is what the runtime construction
    // path uses.

    quote! {
        // Lints that mirror RUBY-source properties, not codegen defects: an
        // unused Ruby assignment, code after a `raise`, Kernel#URI's own
        // capitalization. Genuine-defect lints (unused_must_use and friends)
        // stay live -- the generated program is expected to build clean.
        #![allow(
            unused_parens,
            unused_braces,
            unused_mut,
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

        #(#classes)*
        #(#class_method_containers)*
        #(#builtin_reopens)*
        #(#um_containers)*
        #(#exc_containers)*
        #(#own_bridge_containers)*
        #(#sst_containers)*

        fn main() {
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
            zeo_rt::install_class_registry(__registry);
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
            // Ruby's own PARSE-time warnings (a duplicated hash key, ...),
            // collected by the front end and replayed before the program's
            // first line -- where CRuby prints them.
            #parse_warnings
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
/// propagates its `Signal` through `?` like any method-body statement.
/// Runs right after this class/module's own dispatch-table registration.
pub(crate) fn emit_class_body_site(
    compiler: &Compiler,
    site: &crate::compiler::ClassBodySite,
) -> TokenStream {
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
    let stmts = &site.stmts;
    if stmts.is_empty() {
        return quote! { #alias_check };
    }
    let label_counter = Cell::new(0u32);
    // A class body is an ordinary Ruby scope with ordinary locals, and an
    // escaping block written in it closes over them exactly as one written at
    // the top level does (`yesno = CompletingHash.new; %w[- no].each { |el|
    // yesno[el] = false }`, optparse's own accept-table setup). Left empty,
    // every such name was re-declared nil INSIDE the closure.
    let captures =
        captures::collect_escaping_captures(compiler, stmts, &crate::hir::Params::default(), None);
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
        captured_locals: std::borrow::Cow::Borrowed(&captures.locals),
        self_ident: format_ident!("self"),
        in_real_proc: false,
        self_is_dynamic: false,
        runtime_super_params: None,
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
            let kind = if compiler.class(cid).is_module {
                "module"
            } else {
                "class"
            };
            let label = format!("<{kind}:{}>", compiler.leaf_name(cid));
            quote! { let __frame = zeo_rt::FrameGuard::push(#file, #label, #line); }
        }
        None => quote! {},
    };
    quote! { { #frame #body }?; #alias_check }
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
                if seen.insert(s) {
                    if let Some(site) = site_by_marker.get(&s) {
                        work.extend(site.stmts.iter().copied());
                    }
                }
            }
            crate::hir::HirNode::BoxScope { body, .. } => work.extend(body.iter().copied()),
            _ => {}
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
        .map(|&sid| emit_class_method_fn(compiler, sid));
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
    let no_captures =
        captures::collect_escaping_captures(compiler, &scope.body, &scope.params, None);
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
    // `check_ints` after the frame push: the method-prologue interruption
    // checkpoint (pairs with the back-edge check in `loops`), so recursion-
    // driven busy work is killable even with no native loop in sight.
    quote! {
        #[allow(unused_variables)]
        pub fn #method_ident(#sig_params) -> Result<zeo_rt::RubyValue, zeo_rt::Signal> {
            #frame
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
/// materializes off `own_methods` at compile time). Returns (containers, regs);
/// the regs run AFTER the module's own `__registry.register` (its entry must
/// exist first -- `define_value_method` asserts it).
fn emit_user_module_bridges(compiler: &Compiler) -> (Vec<TokenStream>, Vec<TokenStream>) {
    let mut containers = Vec::new();
    let mut regs = Vec::new();
    for (idx, class) in compiler.classes.iter().enumerate() {
        if idx == 0 || class.is_builtin || class.is_bootstrap || !class.is_module {
            continue;
        }
        if class.own_methods.is_empty() || !compiler.feature_active(ClassId(idx as u32)) {
            continue;
        }
        let cid = ClassId(idx as u32);
        let id = idx as u32;
        let flat = class.name.replace("::", "_");
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
            let key = &scope.name;
            regs.push(quote! {
                __registry.define_value_method(
                    zeo_rt::ClassId(#id),
                    0u32,
                    zeo_rt::Symbol::intern(#key),
                    #tramp,
                );
            });
        }
    }
    (containers, regs)
}

fn emit_builtin_reopen(compiler: &Compiler, cid: ClassId) -> TokenStream {
    let ci = compiler.class(cid);
    let mod_ident = ident::class_ident(compiler, cid);
    let instance_fns = ci
        .methods
        .iter()
        .map(|&sid| emit_builtin_method_fn(compiler, cid, sid));
    // Instance and class methods share this one container but not their
    // idents (`x` vs `__cm_x`), so a reopen defining both -- `module Kernel;
    // def URI(u); end; module_function :URI; end`, which is what
    // `module_function` produces -- emits cleanly.
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
            let tramp = params::emit_exc_trampoline(
                &fn_path,
                name,
                &scope.params,
                scope.needs_block_param(),
                &scope_frame_guard(compiler, scope, false),
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
    let scope = compiler.scope(sid);
    let method_ident = safe_ident(&scope.name);
    let needs_block = scope.needs_block_param();
    let sig_params = params::emit_signature_params(&scope.params, needs_block);
    let label_counter = Cell::new(0u32);
    let method_captures =
        captures::collect_escaping_captures(compiler, &scope.body, &scope.params, Some(cid));
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
        // ivars are name-keyed with storage in `dispatch::Object`.
        self_is_dynamic: true,
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
            zeo_rt::check_ints()?;
            #body_tokens
        }
    }
}

fn emit_class(compiler: &Compiler, cid: ClassId) -> TokenStream {
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

    let methods = ci.methods.iter().map(|&sid| {
        let scope = compiler.scope(sid);
        let method_ident = safe_ident(&scope.name);
        let needs_block = scope.needs_block_param();
        let sig_params = params::emit_signature_params(&scope.params, needs_block);
        let method_label_counter = Cell::new(0u32);
        let method_captures =
            captures::collect_escaping_captures(compiler, &scope.body, &scope.params, Some(cid));
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
        let body_tokens = wrap_method_return(needs_return_catch, quote! { #prologue #body });
        let frame = scope_frame_guard(compiler, scope, false);
        quote! {
            def #method_ident(self: std::sync::Arc<Self> #sig_params) {
                #frame
                zeo_rt::check_ints()?;
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
            &scope_frame_guard(compiler, scope, false),
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
        zeo_rt::ruby_class! {
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
