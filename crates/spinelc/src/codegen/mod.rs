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
mod collections;
mod expr;
mod hoisting;
mod ident;
mod loops;
mod stmt;

use quote::quote;

use crate::analyze::Analyzed;
use crate::compiler::{ClassId, Compiler, OBJECT_CLASS};
use crate::types::TyKind;
use ident::safe_ident;
use proc_macro2::TokenStream;
use std::cell::Cell;
use std::collections::HashMap;
use syn::Lifetime;

#[derive(Clone)]
struct Ctx<'a> {
    compiler: &'a Compiler,
    current_class: Option<ClassId>,
    current_method: Option<String>,
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

    let classes = compiler
        .classes
        .iter()
        .enumerate()
        .filter(|&(idx, _)| idx != 0) // Object -- built into spinel-rt, not user-defined
        .map(|(idx, _)| emit_class(compiler, ClassId(idx as u32)));

    let registrations = compiler
        .classes
        .iter()
        .enumerate()
        .filter(|&(idx, _)| idx != 0)
        .map(|(_, class)| {
            let ident = safe_ident(&class.name);
            quote! { #ident::__register(&mut __registry); }
        });

    let main_label_counter = Cell::new(0u32);
    let cx = Ctx {
        compiler,
        current_class: None,
        current_method: None,
        local_types: &analyzed.main_local_types,
        label_counter: &main_label_counter,
        loop_labels: None,
        for_var_override: None,
    };
    let main_body = hoisting::emit_hoisted_body(&cx, &analyzed.main_statements, true);

    quote! {
        #(#classes)*

        fn main() {
            let mut __registry = spinel_rt::ClassRegistry::new();
            __registry.register(spinel_rt::Object::CLASS_ID, None);
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
    let ivar_idents = ci.ivars.iter().map(|iv| safe_ident(iv));

    let methods = ci.methods.iter().map(|&sid| {
        let scope = compiler.scope(sid);
        let method_ident = safe_ident(&scope.name);
        let params = scope.params.iter().map(|p| {
            let p_ident = safe_ident(p);
            quote! { , #p_ident: spinel_rt::RubyValue }
        });
        let method_label_counter = Cell::new(0u32);
        let method_cx = Ctx {
            compiler,
            current_class: Some(cid),
            current_method: Some(scope.name.clone()),
            local_types: &scope.local_types,
            label_counter: &method_label_counter,
            loop_labels: None,
            for_var_override: None,
        };
        let body = hoisting::emit_hoisted_body(&method_cx, &scope.body, true);
        quote! {
            def #method_ident(&self #(#params)*) {
                #body
            }
        }
    });

    quote! {
        spinel_rt::ruby_class! {
            class #name_ident : #parent_ty {
                id: #id;
                ivars { #(#ivar_idents),* }
                #(#methods)*
            }
        }
    }
}
