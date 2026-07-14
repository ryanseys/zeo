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
mod expr;
mod hoisting;
mod ident;
mod loops;
mod params;
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
    /// The enclosing method/top-level scope's per-local static types (see
    /// `analyze::locals`) -- lets operator dispatch resolve `x + y` to
    /// native `Int` arithmetic for locals, not just literal operands.
    local_types: &'a HashMap<String, TyKind>,
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
    /// (`Rc<RefCell<RubyValue>>`) storage class instead of a plain hoisted
    /// `let mut` (see `hoisting::local_storage`), computed ONCE per method/
    /// top-level scope, same lifetime as `local_types`.
    captured_locals: &'a HashSet<String>,
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
}

impl<'a> Ctx<'a> {
    /// A child context for a native loop's own body -- see `loop_labels`'s
    /// docs.
    fn in_loop(&self, redo: Lifetime, outer: Lifetime) -> Ctx<'a> {
        Ctx {
            loop_labels: Some((redo, outer)),
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

    /// A child context for a real escaping `Proc` closure's own body -- see
    /// `in_real_proc`/`self_ident`'s docs. `loop_labels`/`for_var_override`
    /// reset to `None`: a `break`/`next`/`redo`/`for`-variable lexically
    /// inside the closure is never targeting a loop OUTSIDE it (a Rust
    /// closure is its own function boundary, unlike inlined splices).
    // Not consumed yet -- wired up together with Proc construction, later in
    // this same phase.
    #[allow(dead_code)]
    fn in_proc(&self, needs_self_capture: bool) -> Ctx<'a> {
        Ctx {
            loop_labels: None,
            for_var_override: None,
            self_ident: if needs_self_capture {
                format_ident!("__self")
            } else {
                self.self_ident.clone()
            },
            in_real_proc: true,
            ..self.clone()
        }
    }
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

    // `Object` (index 0, built into `spinel-rt`) and every MODULE never get
    // a generated Rust struct/`impl RubyObject`/`ClassRegistry` entry at
    // all -- a module's methods only ever manifest indirectly, MATERIALIZED
    // onto whatever includes/prepends/extends it (see the plan's Part 6 and
    // `ruby_class!`'s docs).
    let classes = compiler
        .classes
        .iter()
        .enumerate()
        .filter(|&(idx, class)| idx != 0 && !class.is_module)
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
        .filter(|&(idx, class)| idx != 0 && !class.class_methods.is_empty())
        .map(|(idx, _)| emit_class_methods(compiler, ClassId(idx as u32)));

    // In FILE order (matching real Ruby's "a class/module body runs
    // immediately as it's defined"): register the class's dispatch table
    // (skipped for a module, which has none), then run any class-body
    // top-level `@@x = expr` statements -- including a MODULE's own, which
    // still needs to run even though a module never gets a `__register()`
    // call of its own (see `ClassInfo::class_body_stmts`'s docs).
    let registrations = compiler
        .classes
        .iter()
        .enumerate()
        .filter(|&(idx, _)| idx != 0)
        .map(|(idx, class)| {
            let register = if class.is_module {
                quote! {}
            } else {
                let ident = safe_ident(&class.name);
                quote! { #ident::__register(&mut __registry); }
            };
            let class_body = emit_class_body_stmts(compiler, ClassId(idx as u32));
            quote! { #register #class_body }
        });

    let main_label_counter = Cell::new(0u32);
    let main_captures = captures::collect_escaping_captures(compiler, &analyzed.main_statements);
    let cx = Ctx {
        compiler,
        current_class: None,
        defining_class: None,
        current_method: None,
        local_types: &analyzed.main_local_types,
        label_counter: &main_label_counter,
        loop_labels: None,
        for_var_override: None,
        captured_locals: &main_captures.locals,
        self_ident: format_ident!("self"),
        in_real_proc: false,
    };
    let main_body = hoisting::emit_hoisted_body(&cx, &analyzed.main_statements, true);

    quote! {
        #(#classes)*
        #(#class_method_containers)*

        fn main() {
            let mut __registry = spinel_rt::ClassRegistry::new();
            __registry.register(spinel_rt::Object::CLASS_ID, vec![spinel_rt::Object::CLASS_ID]);
            #(#registrations)*
            spinel_rt::install_class_registry(__registry);

            let __result: Result<spinel_rt::RubyValue, spinel_rt::Signal> = (|| {
                #main_body
            })();
            if let Err(__signal) = __result {
                eprintln!("uncaught signal escaped the top level: {:?}", __signal);
                std::process::exit(1);
            }
        }
    }
}

/// See `ClassInfo::class_body_stmts`'s docs -- only a bare `@@x = expr`
/// written directly in a class/module body, run once (for its side effect
/// on cvar storage) from generated `main()`, in file order, right after
/// this class/module's own dispatch-table registration (if any). No
/// `Signal`/`Result` propagation exists at this position (unlike an
/// ordinary method body) -- an expression here that needed `?` would be a
/// compile-time error, not silent wrongness, and nothing in this narrow,
/// documented scope-cut needs one.
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
        current_class: Some(cid),
        defining_class: Some(cid),
        current_method: None,
        local_types: &no_locals,
        label_counter: &label_counter,
        loop_labels: None,
        for_var_override: None,
        captured_locals: &no_captures,
        self_ident: format_ident!("self"),
        in_real_proc: false,
    };
    let exprs = stmts.iter().map(|&id| expr::emit_expr(&cx, id));
    quote! { #(#exprs;)* }
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
/// **Explicit scope-cut**: only plain required parameters are supported
/// (optional/rest/post/keyword/block are a clean rejection, in
/// `emit_class_method_fn` below), and a class method's own body may not
/// reference `self`/`@ivar` at all -- there's no concrete instance for
/// `self` to mean here (a class-level ivar / `class << self` state store
/// is real future work, not attempted this phase; see the plan's Part 6).
fn emit_class_methods(compiler: &Compiler, cid: ClassId) -> TokenStream {
    let ci = compiler.class(cid);
    let name_ident = safe_ident(&ci.name);
    let fns = ci.class_methods.iter().map(|&sid| emit_class_method_fn(compiler, sid));
    if ci.is_module {
        // Ruby module names are conventionally PascalCase (matching a Rust
        // struct/type's own convention), which `rustc` otherwise flags as
        // non-idiomatic for a `mod` (conventionally snake_case) -- silenced
        // rather than renamed, since call sites (`ModuleName::method(...)`)
        // must match the Ruby-visible name exactly.
        quote! {
            #[allow(non_snake_case)]
            pub mod #name_ident { #(#fns)* }
        }
    } else {
        quote! { impl #name_ident { #(#fns)* } }
    }
}

fn emit_class_method_fn(compiler: &Compiler, sid: crate::compiler::ScopeId) -> TokenStream {
    let scope = compiler.scope(sid);
    let mut ivars = Vec::new();
    for &n in &scope.body {
        crate::analyze::collect_ivars(&compiler.hir, n, &mut ivars);
    }
    if !ivars.is_empty() {
        panic!(
            "class method `{}` references `@{}` -- `self`/instance-variable access inside a class method (`def self.x`, or a module method pulled in via `extend`) isn't supported yet (spike scope, no class-level ivar/`class << self` state store exists)",
            scope.name, ivars[0]
        );
    }
    let params = &scope.params;
    if !params.optional.is_empty()
        || params.rest.is_some()
        || !params.post.is_empty()
        || !params.keywords.is_empty()
        || params.keyword_rest.is_some()
        || params.block.is_some()
        || scope.uses_bare_block
    {
        panic!(
            "class method `{}` uses optional/rest/post/keyword parameters or a block -- only plain required parameters are supported yet for class methods (spike scope)",
            scope.name
        );
    }

    let method_ident = safe_ident(&scope.name);
    let sig_params = params.required.iter().map(|n| {
        let ident = safe_ident(n);
        quote! { #ident: spinel_rt::RubyValue }
    });
    let label_counter = Cell::new(0u32);
    let no_captures = captures::collect_escaping_captures(compiler, &scope.body);
    let cx = Ctx {
        compiler,
        // No concrete receiver exists for a class method (no `self:
        // Rc<Self>`) -- but `defining_class` (which class/module this body
        // was LEXICALLY written in) still needs to be real, for `@@cvar`
        // ownership lookup (`codegen::expr::cvar_owner_id`).
        current_class: None,
        defining_class: Some(scope.defining_class),
        current_method: Some(scope.name.clone()),
        local_types: &scope.local_types,
        label_counter: &label_counter,
        loop_labels: None,
        for_var_override: None,
        captured_locals: &no_captures.locals,
        self_ident: format_ident!("self"),
        in_real_proc: false,
    };
    let body = hoisting::emit_hoisted_body(&cx, &scope.body, true);
    quote! {
        pub fn #method_ident(#(#sig_params),*) -> Result<spinel_rt::RubyValue, spinel_rt::Signal> {
            #body
        }
    }
}

fn emit_class(compiler: &Compiler, cid: ClassId) -> TokenStream {
    let ci = compiler.class(cid);
    let name_ident = safe_ident(&ci.name);
    let parent = ci.parent.unwrap_or(OBJECT_CLASS);
    let parent_ty = if parent == OBJECT_CLASS {
        quote! { spinel_rt::Object }
    } else {
        let parent_ident = safe_ident(&compiler.class(parent).name);
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
        let method_captures = captures::collect_escaping_captures(compiler, &scope.body);
        let method_cx = Ctx {
            compiler,
            current_class: Some(cid),
            defining_class: Some(scope.defining_class),
            current_method: Some(scope.name.clone()),
            local_types: &scope.local_types,
            label_counter: &method_label_counter,
            loop_labels: None,
            for_var_override: None,
            captured_locals: &method_captures.locals,
            self_ident: format_ident!("self"),
            in_real_proc: false,
        };
        let prologue = params::emit_prologue(&method_cx, &scope.params);
        let body = hoisting::emit_hoisted_body(&method_cx, &scope.body, true);
        // The `Signal::Return` catch is needed ONLY when this method's OWN
        // body lexically contains an escaping block -- confirmed the hard
        // way NOT to be "wrap every method unconditionally" (a simpler
        // design tried first): a method with no escaping block of its own
        // (e.g. one that just does `yield` to whatever block it's handed)
        // must NOT catch `Signal::Return` in transit, or it would
        // incorrectly intercept a `return` meant for a DIFFERENT method --
        // wherever the block it's currently invoking was actually written --
        // turning "return from the caller" into "this method returns
        // normally instead". See `codegen::captures::body_contains_escaping_block`'s
        // docs.
        let body_tokens = if captures::body_contains_escaping_block(compiler, &scope.body) {
            quote! {
                (|| -> Result<spinel_rt::RubyValue, spinel_rt::Signal> {
                    #prologue
                    #body
                })().or_else(|__e| match __e {
                    spinel_rt::Signal::Return(__v) => Ok(__v),
                    __e => Err(__e),
                })
            }
        } else {
            quote! {
                #prologue
                #body
            }
        };
        quote! {
            def #method_ident(self: std::rc::Rc<Self> #sig_params) {
                #body_tokens
            }
        }
    });

    let dispatch_entries = ci.methods.iter().map(|&sid| {
        let scope = compiler.scope(sid);
        let method_ident = safe_ident(&scope.name);
        let tramp = params::emit_dynamic_trampoline(
            &name_ident,
            &scope.name,
            &scope.params,
            scope.needs_block_param(),
        );
        quote! { #method_ident => #tramp }
    });

    quote! {
        spinel_rt::ruby_class! {
            class #name_ident : #parent_ty {
                id: #id;
                ancestors: [ #(#ancestor_ids),* ];
                ivars { #(#ivar_idents),* }
                #(#methods)*
                dispatch { #(#dispatch_entries),* }
            }
        }
    }
}
