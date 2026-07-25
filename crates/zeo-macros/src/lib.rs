//! The `ruby_class! { ... }` / `ruby_module! { ... }` proc-macros.
//!
//! Two macros mirroring Ruby's own `class`/`module` keywords: a class file
//! opens `ruby_class! { NAME = ID < SUPER; ... }`, a module file opens
//! `ruby_module! { NAME = ID; ... }`. They share one expansion (the kind only
//! feeds the deferred shape projection), so the emitted code is identical given
//! the same methods/constants.
//!
//! Parses the DSL with the shared [`zeo_class_spec`] grammar and emits, for one
//! Ruby core class/module, everything the RUNTIME needs -- drift-free with the
//! hand-written `builtin_methods!` it replaces:
//!
//! - one real Rust `fn` per `def` (a named frame in backtraces, unit-testable);
//! - the instance and class method LOOKUP tables (`lookup`/`lookup_names`/
//!   `lookup_arity` and the `lookup_class*` trio), the same surfaces
//!   `builtin_methods!` derives from one row set;
//! - `install_constants`, which seeds each `const` via `constants::const_set`;
//! - a `linkme` registration into `builtins::BUILTIN_TABLES` keyed by the
//!   class's `ClassId`, so the runtime auto-collects the table instead of a
//!   hand-maintained `match` in `builtins/mod.rs`.
//!
//! Everything is emitted into the invoking module against `crate::...` paths
//! (`crate::RubyValue`, `crate::builtins::BuiltinMethodFn`, ...), exactly like
//! `builtin_methods!`, so a class file just calls `ruby_class! { ... }`.
//!
//! A body may NEST `class`/`module` items with a braced body (mirroring Ruby's
//! `module Process; class Status; end; end`). Each nested class expands
//! recursively into its own private submodule (`use super::*` re-exposes the
//! file's helpers), so several classes can share one file without their fixed
//! table fn names colliding -- the natural home for a class plus the small
//! helper classes it owns (`Process` + `Process::Status` + `Process::Tms`).
//!
//! Phase note: the SHAPE the DSL header declares (module/class, superclass,
//! includes) is parsed but NOT yet emitted here -- during migration the shape
//! still lives in `zeo_abi::BUILTINS` (the compiler asserts `ClassId`
//! contiguity, so rows can't be removed yet). It is reserved for the build.rs
//! `CLASS_SURFACE` projection, which re-parses these same headers.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::parse::Parser;
use syn::Ident;
use zeo_class_spec::ClassSpec;

/// A Ruby class: `ruby_class! { Float = FLOAT_CLASS < NUMERIC_CLASS; def ... }`.
#[proc_macro]
pub fn ruby_class(input: TokenStream) -> TokenStream {
    match ClassSpec::parse_class.parse(input) {
        Ok(spec) => expand(&spec).into(),
        Err(e) => e.to_compile_error().into(),
    }
}

/// A Ruby module: `ruby_module! { Comparable = COMPARABLE_CLASS; def ... }`.
#[proc_macro]
pub fn ruby_module(input: TokenStream) -> TokenStream {
    match ClassSpec::parse_module.parse(input) {
        Ok(spec) => expand(&spec).into(),
        Err(e) => e.to_compile_error().into(),
    }
}

/// One resolved method entry: a Ruby name bound to the Rust fn that implements
/// it (shared across a `def`'s aliases) and its declared arity.
#[derive(Clone)]
struct Entry {
    ruby: String,
    fn_ident: Ident,
    arity: Option<i64>,
}

fn expand(spec: &ClassSpec) -> TokenStream2 {
    let id = &spec.id;

    // One fn item per `def`; each of its names becomes an Entry pointing at it,
    // split into instance vs class tables. A late `alias` adds another Entry
    // reusing the target's fn + arity.
    let mut fn_items: Vec<TokenStream2> = Vec::new();
    let mut instance: Vec<Entry> = Vec::new();
    let mut class: Vec<Entry> = Vec::new();
    let mut alias_errors: Vec<TokenStream2> = Vec::new();

    for (idx, method) in spec.methods.iter().enumerate() {
        // An explicit `as X` gives the impl a callable Rust name (so sibling
        // bodies can call it directly); otherwise a mangled, unreachable ident.
        let fn_ident = match &method.bound_name {
            Some(bound) => bound.clone(),
            None => mangle(&method.names[0].ruby, idx),
        };
        let (recv, args, block) = (&method.recv, &method.args, &method.block);
        let body = &method.body;
        fn_items.push(quote! {
            pub(crate) fn #fn_ident(
                #recv: &crate::RubyValue,
                #args: &[crate::RubyValue],
                #block: Option<crate::RubyValue>,
            ) -> Result<crate::RubyValue, crate::Signal> {
                #body
            }
        });
        // A `module_function` lands in BOTH tables (instance + class); an
        // ordinary method lands in exactly one, chosen by `def` vs `def self.`.
        for name in &method.names {
            let entry = Entry {
                ruby: name.ruby.clone(),
                fn_ident: fn_ident.clone(),
                arity: name.arity,
            };
            if method.is_module_function {
                instance.push(entry.clone());
                class.push(entry);
            } else if method.is_class_method {
                class.push(entry);
            } else {
                instance.push(entry);
            }
        }
    }

    for alias in &spec.aliases {
        // Resolve against instance methods first, then class methods -- the same
        // name can't be both, and instance is the common case.
        let target = instance
            .iter()
            .chain(class.iter())
            .find(|e| e.ruby == alias.old_name);
        match target {
            Some(t) => {
                let entry = Entry {
                    ruby: alias.new_name.clone(),
                    fn_ident: t.fn_ident.clone(),
                    arity: t.arity,
                };
                // Mirror the target's bucket.
                if class.iter().any(|e| e.ruby == alias.old_name) {
                    class.push(entry);
                } else {
                    instance.push(entry);
                }
            }
            None => {
                let msg = format!(
                    "alias `{}` targets `{}`, which this ruby_class! does not define",
                    alias.new_name, alias.old_name
                );
                alias_errors.push(quote! { compile_error!(#msg); });
            }
        }
    }

    let (instance_items, instance_table) =
        gen_method_table(&instance, "lookup", "lookup_names", "lookup_arity");
    let (class_items, class_table) = gen_method_table(
        &class,
        "lookup_class",
        "lookup_class_names",
        "lookup_class_arity",
    );

    // Constant installer + the table's `install_constants` slot.
    let (const_items, install_slot) = if spec.consts.is_empty() {
        (quote! {}, quote! { None })
    } else {
        let sets = spec.consts.iter().map(|c| {
            let name = c.name.to_string();
            let value = &c.value;
            quote! { crate::constants::const_set(#id.0, #name, { #value }); }
        });
        (
            quote! {
                pub(crate) fn install_constants() {
                    #( #sets )*
                }
            },
            quote! { Some(install_constants) },
        )
    };

    // The linkme registration. A unique static name per class so multiple
    // ruby_class! invocations (each in its own module) never collide;
    // upper-cased since it's a static.
    let register_ident =
        format_ident!("__RUBY_CLASS_TABLE_{}", spec.name.to_string().to_uppercase());
    let register = quote! {
        #[linkme::distributed_slice(crate::builtins::BUILTIN_TABLES)]
        static #register_ident: crate::builtins::BuiltinClassTable =
            crate::builtins::BuiltinClassTable {
                id: #id,
                instance: #instance_table,
                class: #class_table,
                install_constants: #install_slot,
            };
    };

    // Nested classes/modules (`class Status = … { … }` inside this body) each
    // expand recursively into their OWN private submodule, so the fixed table
    // fn names (`lookup`/`lookup_class`/…) never collide with this class's or a
    // sibling's. `use super::*` re-exposes the file's helper fns and imports the
    // nested bodies rely on; the `linkme` registration works from any module.
    let nested_mods = spec.nested.iter().map(|n| {
        let mod_ident = format_ident!("__ruby_class_{}", n.name.to_string().to_lowercase());
        let inner = expand(n);
        quote! {
            #[allow(non_snake_case)]
            mod #mod_ident {
                use super::*;
                #inner
            }
        }
    });

    quote! {
        #( #fn_items )*
        #instance_items
        #class_items
        #const_items
        #register
        #( #alias_errors )*
        #( #nested_mods )*
    }
}

/// Generate the `(lookup, names, arity)` trio for one bucket of entries and the
/// `Option<MethodTable>` that points at them. Empty bucket -> no items, `None`.
fn gen_method_table(
    entries: &[Entry],
    lookup: &str,
    names: &str,
    arity: &str,
) -> (TokenStream2, TokenStream2) {
    if entries.is_empty() {
        return (quote! {}, quote! { None });
    }
    let lookup_fn = format_ident!("{lookup}");
    let names_fn = format_ident!("{names}");
    let arity_fn = format_ident!("{arity}");

    let lookup_arms = entries.iter().map(|e| {
        let (ruby, fn_ident) = (&e.ruby, &e.fn_ident);
        quote! { #ruby => Some(#fn_ident), }
    });
    let name_lits = entries.iter().map(|e| {
        let ruby = &e.ruby;
        quote! { #ruby }
    });
    let arity_arms = entries.iter().map(|e| {
        let ruby = &e.ruby;
        let a = e.arity.unwrap_or(-1);
        quote! { #ruby => Some(#a), }
    });

    let items = quote! {
        pub(crate) fn #lookup_fn(name: &str) -> Option<crate::builtins::BuiltinMethodFn> {
            match name {
                #( #lookup_arms )*
                _ => None,
            }
        }
        #[allow(dead_code)]
        pub(crate) fn #names_fn() -> &'static [&'static str] {
            &[ #( #name_lits ),* ]
        }
        #[allow(dead_code)]
        pub(crate) fn #arity_fn(name: &str) -> Option<i64> {
            match name {
                #( #arity_arms )*
                _ => None,
            }
        }
    };
    let table = quote! {
        Some(crate::builtins::MethodTable {
            lookup: #lookup_fn,
            names: #names_fn,
            arity: #arity_fn,
        })
    };
    (items, table)
}

/// A deterministic, valid, collision-free Rust fn ident for a Ruby method name.
///
/// Ruby names carry operators and `?`/`!` that aren't legal in idents, so map
/// each to a word; the `_<idx>` suffix (the def's position) guarantees
/// uniqueness even when two names sanitize alike. The `rc_` prefix keeps these
/// clear of the class file's own helper fns. Readability is a bonus (backtrace
/// frames), not load-bearing -- these fns are only reached through the lookup
/// tables, never called by Rust name.
fn mangle(primary: &str, idx: usize) -> Ident {
    let mut s = String::new();
    for ch in primary.chars() {
        match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' => s.push(ch),
            '<' => s.push_str("lt"),
            '>' => s.push_str("gt"),
            '=' => s.push_str("eq"),
            '+' => s.push_str("plus"),
            '-' => s.push_str("minus"),
            '*' => s.push_str("star"),
            '/' => s.push_str("slash"),
            '%' => s.push_str("pct"),
            '!' => s.push_str("bang"),
            '?' => s.push_str("eh"),
            '[' => s.push_str("idx"),
            ']' => {}
            '~' => s.push_str("tilde"),
            '&' => s.push_str("amp"),
            '|' => s.push_str("pipe"),
            '^' => s.push_str("caret"),
            '@' => s.push_str("at"),
            _ => s.push('_'),
        }
    }
    if s.is_empty() || s.starts_with(|c: char| c.is_ascii_digit()) {
        s.insert_str(0, "m_");
    }
    format_ident!("rc_{s}_{idx}")
}
