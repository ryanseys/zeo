//! The `ruby_class! { ... }` / `ruby_module! { ... }` proc-macros.
//!
//! Two macros mirroring Ruby's own `class`/`module` keywords: a class file
//! opens `ruby_class! { NAME = ID < SUPER; ... }`, a module file opens
//! `ruby_module! { NAME = ID; ... }`. They share one expansion (the kind only
//! feeds the deferred shape projection), so the emitted code is identical given
//! the same methods/constants.
//!
//! Parses the DSL with the shared [`zeo_dsl`] grammar and emits, for one
//! Ruby core class/module, everything the RUNTIME needs:
//!
//! - one real Rust `fn` per `def` (a named frame in backtraces, unit-testable);
//! - the instance and class method LOOKUP tables (`lookup`/`lookup_names`/
//!   `lookup_arity` and the `lookup_class*` trio), derived from one row set;
//! - `install_constants`, which seeds each `const` via `constants::const_set`;
//! - a `linkme` registration into `builtins::BUILTIN_TABLES` keyed by the
//!   class's `ClassId`, so the runtime auto-collects the table instead of a
//!   hand-maintained `match` in `builtins/mod.rs`.
//!
//! Everything is emitted into the invoking module against `crate::...` paths
//! (`crate::RubyValue`, `crate::builtins::BuiltinMethodFn`, ...), so a class
//! file just calls `ruby_class! { ... }`.
//!
//! A body may NEST `class`/`module` items with a braced body (mirroring Ruby's
//! `module Process; class Status; end; end`). Each nested class expands
//! recursively into its own private submodule (`use super::*` re-exposes the
//! file's helpers), so several classes can share one file without their fixed
//! table fn names colliding -- the natural home for a class plus the small
//! helper classes it owns (`Process` + `Process::Status` + `Process::Tms`).
//!
//! The SHAPE the DSL header declares (module/class, superclass, includes) is
//! parsed but not emitted here: it feeds the build.rs `CLASS_SURFACE`
//! projection, which re-parses these same headers, while `zeo_abi::BUILTINS`
//! remains the compiler's source of `ClassId` contiguity.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::Ident;
use syn::parse::Parser;
use zeo_dsl::ClassSpec;

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
    /// What `Method#arity` reports: the def's parameter list, unless this name
    /// carries an explicit override.
    arity: i64,
    /// Outer attributes (`#[cfg(...)]`) gating this entry -- shared with the
    /// impl fn so a cfg'd-out method drops its fn AND its table rows together.
    attrs: Vec<syn::Attribute>,
    /// `private def`, or the INSTANCE half of a `module_function` -- CRuby
    /// makes that copy private, which is why `Math.instance_methods(false)` is
    /// empty while `Math.private_instance_methods(false)` has 28.
    is_private: bool,
    /// See `zeo_dsl::MethodDef::allocs` -- CLASS-method rows only.
    allocs: bool,
    /// See `zeo_dsl::MethodDef::inherits` -- the row dispatches here, but ruby
    /// names an ancestor as its owner.
    inherits: bool,
    /// `protected def` -- reachable only from a receiver the caller is a kind
    /// of. Never set by `module_function`, which splits private/public only.
    is_protected: bool,
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
        // A named fn is `pub`, not `pub(crate)`: naming it is a request to call
        // it from elsewhere, and codegen's Kernel fast path reaches
        // `zeo_rt::kernel_integer` and friends by exactly this route -- which
        // is what keeps the fast path and the dispatch row one function, so
        // they cannot disagree about the argument count.
        let (fn_ident, fn_vis) = match &method.bound_name {
            Some(bound) => (bound.clone(), quote! { pub }),
            None => (mangle(&method.names[0].ruby, idx), quote! { pub(crate) }),
        };
        let recv = &method.recv;
        let body = &method.body;
        let attrs = &method.attrs;
        let preamble = gen_preamble(method);
        // `block_or_enum!`'s blockless arm needs the Ruby name to build the
        // Enumerator that re-invokes this very method. Every call site used to
        // restate it as a literal beside the def that owns it -- pure
        // repetition, and a typo there yields an enumerator over the WRONG
        // method. Bind it here instead, only for the bodies that ask.
        // The unwrapped receiver the class header declares, bound only where a
        // body names it -- the rest keep the untyped slot alone.
        let recv_binding = match &spec.receiver {
            Some((name, variant)) if mentions(&method.body, &name.to_string()) => {
                let msg = format!(
                    "{} table row dispatched on a non-{} receiver",
                    spec.name, spec.name
                );
                quote! {
                    let #name = match #recv {
                        #variant(v) => v,
                        _ => unreachable!(#msg),
                    };
                }
            }
            _ => quote! {},
        };
        // A def with SEVERAL names whose body builds an Enumerator has to know
        // which one was called: CRuby's `RETURN_SIZED_ENUMERATOR` captures
        // `__callee__`, so `[1, 2].collect.inspect` says "collect" and not
        // "map". One body, one thin wrapper per alias passing its own literal
        // -- no thread-local, no widened `BuiltinMethodFn`, and nothing on the
        // hot path. A `bound_name` def keeps the single-fn shape: codegen's
        // fast paths call it by name.
        // A body asks for the callee either by building an Enumerator with
        // `block_or_enum!` or by naming `__RUBY_METHOD` outright (to hand it
        // to a shared helper).
        let wants_callee =
            mentions(&method.body, "block_or_enum") || mentions(&method.body, "__RUBY_METHOD");
        let per_alias_callee =
            method.names.len() > 1 && method.bound_name.is_none() && wants_callee;
        let mut alias_idents: Vec<syn::Ident> = Vec::new();
        if per_alias_callee {
            // One fn per alias, each with its OWN `__RUBY_METHOD`. The name
            // cannot travel as a PARAMETER: `block_or_enum!` is a
            // `macro_rules!`, whose hygiene keeps its `__RUBY_METHOD` from
            // binding to a local introduced anywhere else -- only an ITEM
            // (this `const`) is visible to it.
            for (n, name) in method.names.iter().enumerate() {
                let wrapper = quote::format_ident!("{}_as{}", fn_ident, n);
                let callee = &name.ruby;
                fn_items.push(quote! {
                    #( #attrs )*
                    fn #wrapper(
                        #recv: &crate::RubyValue,
                        __args: &[crate::RubyValue],
                        __block: Option<crate::RubyValue>,
                    ) -> Result<crate::RubyValue, crate::Signal> {
                        #recv_binding
                        const __RUBY_METHOD: &str = #callee;
                        #preamble
                        #body
                    }
                });
                alias_idents.push(wrapper);
            }
        } else {
            let method_name = if wants_callee {
                let primary = &method.names[0].ruby;
                quote! { const __RUBY_METHOD: &str = #primary; }
            } else {
                quote! {}
            };
            fn_items.push(quote! {
                #( #attrs )*
                #fn_vis fn #fn_ident(
                    #recv: &crate::RubyValue,
                    __args: &[crate::RubyValue],
                    __block: Option<crate::RubyValue>,
                ) -> Result<crate::RubyValue, crate::Signal> {
                    #recv_binding
                    #method_name
                    #preamble
                    #body
                }
            });
        }
        // A `module_function` lands in BOTH tables (instance + class); an
        // ordinary method lands in exactly one, chosen by `def` vs `def self.`.
        let derived = method.derived_arity();
        let declared_private = method.visibility == zeo_dsl::Visibility::Private;
        let declared_protected = method.visibility == zeo_dsl::Visibility::Protected;
        for (alias_n, name) in method.names.iter().enumerate() {
            let entry = Entry {
                ruby: name.ruby.clone(),
                fn_ident: alias_idents
                    .get(alias_n)
                    .cloned()
                    .unwrap_or_else(|| fn_ident.clone()),
                arity: name.arity.unwrap_or(derived),
                attrs: method.attrs.clone(),
                is_private: declared_private,
                is_protected: declared_protected,
                allocs: method.allocs,
                // Per-name OR def-wide: `Regexp.new` is Class's while
                // `Regexp.compile` is Regexp's own, and both share one body.
                inherits: method.inherits || name.inherits,
            };
            if method.is_module_function {
                // CRuby's `module_function` splits the visibility: the instance
                // copy is private, the singleton copy public. `Math` is the
                // proof -- 28 private instance methods, 28 public singletons.
                instance.push(Entry {
                    is_private: true,
                    ..entry.clone()
                });
                class.push(Entry {
                    is_private: false,
                    ..entry
                });
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
                    attrs: t.attrs.clone(),
                    is_private: t.is_private,
                    is_protected: t.is_protected,
                    allocs: t.allocs,
                    inherits: t.inherits,
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
            let attrs = &c.attrs;
            let name = c.name.to_string();
            let value = &c.value;
            quote! { #( #attrs )* crate::constants::const_set(#id.0, #name, { #value }); }
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
    let register_ident = format_ident!(
        "__RUBY_CLASS_TABLE_{}",
        spec.name.to_string().to_uppercase()
    );
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

/// The argument-count guard and the parameter bindings a `def`'s signature
/// implies, emitted ahead of the body.
///
/// The guard is the runtime half of what the signature declares; the reported
/// Whether a body's tokens name `ident` anywhere, recursing into groups. Used
/// to bind `__RUBY_METHOD` only where it is read, so the other ~1,400 defs do
/// not carry an unused const.
fn mentions(tokens: &TokenStream2, ident: &str) -> bool {
    tokens.clone().into_iter().any(|t| match t {
        proc_macro2::TokenTree::Ident(i) => i == ident,
        proc_macro2::TokenTree::Group(g) => mentions(&g.stream(), ident),
        _ => false,
    })
}

/// arity (`MethodDef::derived_arity`) is the reflection half. Both come from the
/// one parameter list, so they cannot disagree.
fn gen_preamble(method: &zeo_dsl::MethodDef) -> TokenStream2 {
    use zeo_dsl::ParamKind;

    let min = method.min_args();
    // The guard counts POSITIONALS, so a `**kwrest` slot is excluded -- unlike
    // `max_args`, where CRuby's equation does count it. `Time.new` accepts
    // seven positionals plus `in:`, and CRuby says `expected 0..7` for eight.
    let max = method.rest.is_none().then_some(method.params.len());

    // `**kwrest` peels the trailing options Hash off before anything is
    // counted, so `[1].pack()` reports `given 0`, not `given 1`.
    let (slice, kw_binding) = match &method.kwrest {
        Some(name) => (
            quote! { __pos },
            quote! {
                let (#name, __pos): (Option<&crate::RubyValue>, &[crate::RubyValue]) =
                    match __args.last() {
                        Some(h @ crate::RubyValue::Hash(_)) => (Some(h), &__args[..__args.len() - 1]),
                        _ => (None, __args),
                    };
            },
        ),
        None => (quote! { __args }, quote! {}),
    };

    // A wide-open signature accepts everything, so there is nothing to check --
    // this is what makes `(recv, *args, &block)` byte-for-byte today's code.
    let guard = if min == 0 && max.is_none() {
        quote! {}
    } else {
        let max_tokens = match max {
            Some(n) => quote! { Some(#n) },
            None => quote! { None },
        };
        quote! { crate::builtins::check_arity(#slice.len(), #min, #max_tokens)?; }
    };

    let bindings = method.params.iter().enumerate().map(|(i, p)| {
        let name = &p.name;
        match &p.kind {
            ParamKind::Required => quote! {
                let #name: &crate::RubyValue = &#slice[#i];
            },
            // The default lands in a deferred `let`, so it is evaluated only on
            // the branch that needs it and still yields a borrow.
            ParamKind::Optional(default) => {
                let slot = format_ident!("__default_{i}");
                quote! {
                    let #slot;
                    let #name: &crate::RubyValue = match #slice.get(#i) {
                        Some(v) => v,
                        None => {
                            #slot = { #default };
                            &#slot
                        }
                    };
                }
            }
            ParamKind::Maybe => quote! {
                let #name: Option<&crate::RubyValue> = #slice.get(#i);
            },
        }
    });

    let rest = method.rest.as_ref().map(|name| {
        let from = method.params.len();
        quote! { let #name: &[crate::RubyValue] = &#slice[#from..]; }
    });
    let block = match &method.block {
        Some(name) => quote! { let #name = __block; },
        // The parameter is part of the fixed ABI, so it must look used even
        // when this method ignores the block.
        None => quote! { let _ = &__block; },
    };

    quote! {
        #kw_binding
        #guard
        #( #bindings )*
        #rest
        #block
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
    let private_fn = format_ident!("{lookup}_is_private");
    let protected_fn = format_ident!("{lookup}_is_protected");
    let allocs_fn = format_ident!("{lookup}_allocs");
    let inherits_fn = format_ident!("{lookup}_inherits");

    let lookup_arms = entries.iter().map(|e| {
        let (ruby, fn_ident, attrs) = (&e.ruby, &e.fn_ident, &e.attrs);
        quote! { #( #attrs )* #ruby => Some(#fn_ident), }
    });
    let name_lits = entries.iter().map(|e| {
        let (ruby, attrs) = (&e.ruby, &e.attrs);
        quote! { #( #attrs )* #ruby }
    });
    let arity_arms = entries.iter().map(|e| {
        let (ruby, a, attrs) = (&e.ruby, e.arity, &e.attrs);
        quote! { #( #attrs )* #ruby => Some(#a), }
    });
    // Only the PRIVATE names get an arm; a table with none compiles to
    // `false`, which is every class that declares no `module_function` and no
    // `private def`.
    let private_arms = entries.iter().filter(|e| e.is_private).map(|e| {
        let (ruby, attrs) = (&e.ruby, &e.attrs);
        quote! { #( #attrs )* #ruby => true, }
    });
    let protected_arms = entries.iter().filter(|e| e.is_protected).map(|e| {
        let (ruby, attrs) = (&e.ruby, &e.attrs);
        quote! { #( #attrs )* #ruby => true, }
    });
    // Only the rows marked `allocs` get an arm, so an unmarked row -- and a
    // whole table that marks none, which is nearly all of them -- compiles to
    // `false`: answer the base class. See `zeo_dsl::MethodDef::allocs`.
    let allocs_arms = entries.iter().filter(|e| e.allocs).map(|e| {
        let (ruby, attrs) = (&e.ruby, &e.attrs);
        quote! { #( #attrs )* #ruby => true, }
    });
    // Same shape, and the same default for the same reason: an unmarked row
    // claims ownership. See `zeo_dsl::MethodDef::inherits`.
    let inherits_arms = entries.iter().filter(|e| e.inherits).map(|e| {
        let (ruby, attrs) = (&e.ruby, &e.attrs);
        quote! { #( #attrs )* #ruby => true, }
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
        #[allow(dead_code)]
        pub(crate) fn #private_fn(name: &str) -> bool {
            match name {
                #( #private_arms )*
                _ => false,
            }
        }
        #[allow(dead_code)]
        pub(crate) fn #protected_fn(name: &str) -> bool {
            match name {
                #( #protected_arms )*
                _ => false,
            }
        }
        #[allow(dead_code)]
        pub(crate) fn #allocs_fn(name: &str) -> bool {
            match name {
                #( #allocs_arms )*
                _ => false,
            }
        }
        #[allow(dead_code)]
        pub(crate) fn #inherits_fn(name: &str) -> bool {
            match name {
                #( #inherits_arms )*
                _ => false,
            }
        }
    };
    let table = quote! {
        Some(crate::builtins::MethodTable {
            lookup: #lookup_fn,
            names: #names_fn,
            arity: #arity_fn,
            is_private: #private_fn,
            is_protected: #protected_fn,
            allocs: #allocs_fn,
            inherits: #inherits_fn,
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
