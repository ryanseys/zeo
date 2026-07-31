//! The shared grammar for the `ruby_class! { ... }` DSL.
//!
//! One Ruby core class/module per file (plus any it namespaces via nested
//! `class`/`module` items), declared in a Ruby-like syntax whose method bodies
//! stay real Rust. Parsed here with `syn` so the exact same grammar backs both
//! consumers and they can never drift:
//!
//! - `zeo-macros`' `ruby_class!` proc-macro emits the runtime code (the method
//!   fns, the `ClassId`-keyed lookup tables, the constant installers, the
//!   `linkme` registration).
//! - `zeo`'s build.rs re-parses the same invocations out of the runtime source
//!   and projects `CLASS_SURFACE` -- the shape (name/superclass/includes) and
//!   method/constant NAMES the compiler folds `respond_to?`/`is_a?`/const
//!   lookups against. (It reads only the headers; method bodies are opaque.)
//!
//! Grammar (the opening macro fixes the kind, so the header carries no
//! `module`/`class` keyword -- just `NAME = ID`, plus `< SUPER` for a class):
//! ```text
//! ruby_module! { Comparable = COMPARABLE_CLASS; ... }    // module, own ClassId const
//! ruby_class!  { String = STRING_CLASS < OBJECT_CLASS; ... }  // class + superclass const
//!
//! ruby_module! {
//!     Comparable = COMPARABLE_CLASS;
//!
//!     include ENUMERABLE_CLASS;                          // 0+ mixins (ClassId consts)
//!
//!     const INFINITY = f64::INFINITY;                    // 0+ constants (RHS is a Rust expr)
//!
//!     def "<=>"(recv, args, block) { /* real Rust */ }   // instance method (name: str or ident)
//!     def "between?" arity 2 (recv, args, block) { .. }  // optional per-name arity
//!     def "succ" | "next" (recv, args, block) { .. }     // aliases sharing one body
//!     private def helper(recv, args, block) { .. }       // visibility prefix
//!     def self.pid(recv, args, block) { .. }             // class/singleton method
//!     #[cfg(target_vendor = "apple")] def "change"(..){} // platform-gated def
//!
//!     alias cmp = "<=>";                                 // late alias (new = existing)
//!
//!     class Status = STATUS_CLASS < OBJECT_CLASS {        // nested, braced body
//!         def "exitstatus"(recv, args, block) { .. }      //   -> Process::Status
//!     }
//! }
//! ```
//!
//! Superclass and `include` targets are written as `ClassId` CONSTS (the one
//! hard-ABI token), not names -- so the build.rs projection can emit them
//! symbolically (`zeo_abi::OBJECT_CLASS`) and let rustc resolve them, never
//! evaluating a const itself. Only the header NAME is a plain identifier.

pub mod scan;

use proc_macro2::TokenStream;
use syn::parse::ParseStream;
use syn::{Attribute, Expr, Ident, LitInt, LitStr, Path, Token, braced, parenthesized};

/// A fully-parsed `ruby_class! { ... }` body.
pub struct ClassSpec {
    pub kind: ClassKind,
    /// The Ruby-visible name (`Comparable`, `String`) -- the header identifier.
    pub name: Ident,
    /// The class's own reserved `ClassId` const (e.g. `COMPARABLE_CLASS`), the
    /// single hard-ABI token.
    pub id: Path,
    /// Mixins in source order, as `ClassId` consts.
    pub includes: Vec<Path>,
    pub consts: Vec<ConstDef>,
    pub methods: Vec<MethodDef>,
    pub aliases: Vec<AliasDef>,
    /// Nested classes/modules declared inside this one with a braced body
    /// (`class Status = PROCESS_STATUS_CLASS < OBJECT_CLASS { ... }`), mirroring
    /// Ruby's `module Process; class Status; end; end` namespacing. Each is a
    /// full `ClassSpec` in its own right -- own `ClassId`, methods, constants,
    /// and (recursively) nesting -- that the proc-macro emits into a private
    /// submodule so sibling classes never collide.
    pub nested: Vec<ClassSpec>,
}

/// Module vs class -- a class additionally carries its superclass `ClassId`,
/// except the root (`BasicObject`), whose header omits `< SUPER` (`None`).
pub enum ClassKind {
    Module,
    Class { superclass: Option<Path> },
}

/// `const NAME = <expr>;` -- the value is real Rust the proc-macro passes
/// through; the build.rs projection needs only the name.
pub struct ConstDef {
    /// Outer attributes (`#[cfg(...)]`) gating this constant. A flag constant
    /// often exists on one platform and not another (`Fcntl::F_PREALLOCATE` is
    /// macOS, `F_DUPFD_CLOEXEC` is Linux), and CRuby's own extensions `#ifdef`
    /// each one for exactly that reason.
    pub attrs: Vec<Attribute>,
    pub name: Ident,
    pub value: Expr,
}

/// A method's Ruby-level visibility, mirroring `rb_define_method` vs
/// `rb_define_private_method`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Visibility {
    Public,
    Private,
    Protected,
}

/// One `def`. Several Ruby NAMES can share a single Rust body (aliases that
/// live at the same definition site, e.g. `succ`/`next`); each name carries its
/// own optional arity.
pub struct MethodDef {
    /// `def self.foo` (a class/singleton method) vs `def foo` (instance).
    pub is_class_method: bool,
    /// `module_function def foo` -- a module function: defined as BOTH an
    /// instance method and a class/singleton method (CRuby's `module_function`,
    /// e.g. every `Math.sqrt` also reachable as a private `sqrt` via
    /// `include Math`). The macro emits it into both lookup tables.
    pub is_module_function: bool,
    /// Outer attributes written before the `def` (in practice `#[cfg(...)]`),
    /// carried verbatim onto the emitted fn AND every lookup/names/arity row so
    /// a platform-gated method drops out of the surface as a unit.
    pub attrs: Vec<Attribute>,
    pub visibility: Visibility,
    /// One or more Ruby names, in declaration order (first is the primary).
    pub names: Vec<MethodName>,
    /// An explicit callable Rust fn name for this def (`def "x" as X (...)`),
    /// so sibling method bodies in the same file can call it DIRECTLY by name
    /// (`X(recv, args, blk)`) instead of through the dispatch table. `None`
    /// falls back to the mangled `rc_*` ident (unreachable by Rust name). Only
    /// affects the Rust symbol -- Ruby dispatch (`send`, `respond_to?`) always
    /// resolves by the Ruby name through the lookup table, bound or not.
    pub bound_name: Option<Ident>,
    /// The three body parameters, spelled by the author (`recv`/`_recv`, ...).
    pub recv: Ident,
    pub args: Ident,
    pub block: Ident,
    /// The `{ ... }` body -- real Rust, kept verbatim for the proc-macro.
    pub body: TokenStream,
}

/// One Ruby method name plus its declared `Method#arity` (defaulting to CRuby's
/// variadic `-1` when omitted).
pub struct MethodName {
    pub ruby: String,
    pub arity: Option<i64>,
}

/// `alias new = old;` -- a second name for an already-defined method.
pub struct AliasDef {
    pub new_name: String,
    pub old_name: String,
}

impl ClassSpec {
    /// Parse a `ruby_module!` body: a `NAME = ID;` header (no superclass), then
    /// items. The opening macro, not a keyword, is what fixed the kind.
    pub fn parse_module(input: ParseStream) -> syn::Result<Self> {
        let name: Ident = input.parse()?;
        input.parse::<Token![=]>()?;
        // `parse_mod_style`: the id is a plain `a::b::CONST` path with no
        // generics, so the parser stops at a following `<` rather than mistaking
        // `ID < SUPER` for `ID<SUPER>` generic arguments.
        let id = Path::parse_mod_style(input)?;
        input.parse::<Token![;]>()?;
        Self::finish(input, ClassKind::Module, name, id)
    }

    /// Parse a `ruby_class!` body: a `NAME = ID [< SUPER];` header, then items.
    /// `< SUPER` is optional so the root class `BasicObject` (no superclass) can
    /// use the DSL; every other class declares its superclass.
    pub fn parse_class(input: ParseStream) -> syn::Result<Self> {
        let name: Ident = input.parse()?;
        input.parse::<Token![=]>()?;
        let id = Path::parse_mod_style(input)?;
        let superclass = if input.peek(Token![<]) {
            input.parse::<Token![<]>()?;
            Some(Path::parse_mod_style(input)?)
        } else {
            None
        };
        input.parse::<Token![;]>()?;
        Self::finish(input, ClassKind::Class { superclass }, name, id)
    }

    /// Consume the item list after a header into a finished `ClassSpec`.
    fn finish(input: ParseStream, kind: ClassKind, name: Ident, id: Path) -> syn::Result<Self> {
        let mut spec = ClassSpec {
            kind,
            name,
            id,
            includes: Vec::new(),
            consts: Vec::new(),
            methods: Vec::new(),
            aliases: Vec::new(),
            nested: Vec::new(),
        };
        while !input.is_empty() {
            parse_item(input, &mut spec)?;
        }
        Ok(spec)
    }

    /// Parse a NESTED class/module: a braced-body form the parent's item loop
    /// reaches on a `class`/`module` keyword. `class NAME = ID < SUPER { ... }`
    /// or `module NAME = ID { ... }` -- the header mirrors the top-level one but
    /// the body is `{ ... }`-delimited rather than running to end-of-input.
    fn parse_nested(input: ParseStream) -> syn::Result<Self> {
        let keyword: Ident = input.parse()?; // `class` or `module`
        let name: Ident = input.parse()?;
        input.parse::<Token![=]>()?;
        let id = Path::parse_mod_style(input)?;
        let kind = if keyword == "class" {
            let superclass = if input.peek(Token![<]) {
                input.parse::<Token![<]>()?;
                Some(Path::parse_mod_style(input)?)
            } else {
                None
            };
            ClassKind::Class { superclass }
        } else {
            ClassKind::Module
        };
        let body;
        braced!(body in input);
        Self::finish(&body, kind, name, id)
    }
}

/// Parse one item (after the header) into `spec`. Items are keyword-led:
/// `include`, `const`, `alias`, an optional `private`/`protected` visibility
/// prefix then `def`, or a nested `class`/`module` with a braced body.
fn parse_item(input: ParseStream, spec: &mut ClassSpec) -> syn::Result<()> {
    // Outer attributes (`#[cfg(...)]`) prefix a `def`; only method items carry
    // them, so a stray attribute before anything else is a clear error.
    let attrs = input.call(Attribute::parse_outer)?;
    let attrs_forbidden = |input: ParseStream| {
        syn::Error::new(
            input.span(),
            "attributes are only supported on `def` items (e.g. `#[cfg(...)] def ...`)",
        )
    };
    // `const` is a real Rust keyword, so it can't be peeked as an `Ident` like
    // the (non-keyword) `include`/`alias`/`private`/`protected`/`def` leads.
    if input.peek(Token![const]) {
        input.parse::<Token![const]>()?;
        let name: Ident = input.parse()?;
        input.parse::<Token![=]>()?;
        let value: Expr = input.parse()?;
        input.parse::<Token![;]>()?;
        spec.consts.push(ConstDef { attrs, name, value });
        return Ok(());
    }
    let lookahead: Ident = input.fork().parse().map_err(|_| {
        input.error(
            "expected `include`, `const`, `alias`, `private`, `protected`, `module_function`, `def`, `class`, or `module`",
        )
    })?;
    match lookahead.to_string().as_str() {
        "include" => {
            if !attrs.is_empty() {
                return Err(attrs_forbidden(input));
            }
            input.parse::<Ident>()?; // `include`
            spec.includes.push(Path::parse_mod_style(input)?);
            input.parse::<Token![;]>()?;
        }
        "alias" => {
            if !attrs.is_empty() {
                return Err(attrs_forbidden(input));
            }
            input.parse::<Ident>()?; // `alias`
            let new_name = parse_method_name(input)?;
            input.parse::<Token![=]>()?;
            let old_name = parse_method_name(input)?;
            input.parse::<Token![;]>()?;
            spec.aliases.push(AliasDef { new_name, old_name });
        }
        "private" | "protected" => {
            let vis_ident: Ident = input.parse()?;
            let visibility = if vis_ident == "private" {
                Visibility::Private
            } else {
                Visibility::Protected
            };
            spec.methods.push(parse_def(input, visibility, attrs)?);
        }
        "def" => {
            spec.methods
                .push(parse_def(input, Visibility::Public, attrs)?);
        }
        "module_function" => {
            // `module_function def foo(...)` -- a module function: emitted into
            // BOTH the instance and class tables (CRuby's `module_function`).
            // Private as an instance method, public as a singleton.
            input.parse::<Ident>()?; // `module_function`
            let mut def = parse_def(input, Visibility::Private, attrs)?;
            def.is_module_function = true;
            spec.methods.push(def);
        }
        "class" | "module" => {
            if !attrs.is_empty() {
                return Err(attrs_forbidden(input));
            }
            spec.nested.push(ClassSpec::parse_nested(input)?);
        }
        other => {
            return Err(syn::Error::new(
                lookahead.span(),
                format!("unexpected `{other}` in ruby_class! body"),
            ));
        }
    }
    Ok(())
}

/// Parse a `def` (the `def` keyword is still on `input`), given the visibility
/// and any outer attributes already consumed by the caller.
fn parse_def(
    input: ParseStream,
    visibility: Visibility,
    attrs: Vec<Attribute>,
) -> syn::Result<MethodDef> {
    input.parse::<Ident>()?; // `def`

    // `self .` marks a class/singleton method.
    let is_class_method = input.peek(Token![self]) && input.peek2(Token![.]);
    if is_class_method {
        input.parse::<Token![self]>()?;
        input.parse::<Token![.]>()?;
    }

    // One or more `NAME [arity N]`, separated by `|`, sharing one body.
    let mut names = Vec::new();
    loop {
        let ruby = parse_method_name(input)?;
        let arity = if peek_ident(input, "arity") {
            input.parse::<Ident>()?; // `arity`
            let lit: LitInt = input.parse()?;
            Some(lit.base10_parse::<i64>()?)
        } else {
            None
        };
        names.push(MethodName { ruby, arity });
        if input.peek(Token![|]) {
            input.parse::<Token![|]>()?;
        } else {
            break;
        }
    }

    // Optional `as X`: bind a callable Rust fn name (for direct in-file sibling
    // calls). Comes after all `|`-joined names, before the params.
    let bound_name = if input.peek(Token![as]) {
        input.parse::<Token![as]>()?;
        Some(input.parse::<Ident>()?)
    } else {
        None
    };

    // `(recv, args, block)`.
    let params;
    parenthesized!(params in input);
    let recv: Ident = params.parse()?;
    params.parse::<Token![,]>()?;
    let args: Ident = params.parse()?;
    params.parse::<Token![,]>()?;
    let block: Ident = params.parse()?;

    // The body block, captured verbatim (braces stripped) as real Rust.
    let body_buf;
    braced!(body_buf in input);
    let body: TokenStream = body_buf.parse()?;

    Ok(MethodDef {
        is_class_method,
        is_module_function: false,
        attrs,
        visibility,
        names,
        bound_name,
        recv,
        args,
        block,
        body,
    })
}

/// A Ruby method/alias name: either a string literal (operators, `?`/`!`
/// suffixes -- `"<=>"`, `"between?"`) or a bare identifier (`pid`, `succ`).
fn parse_method_name(input: ParseStream) -> syn::Result<String> {
    if input.peek(LitStr) {
        Ok(input.parse::<LitStr>()?.value())
    } else {
        Ok(input.parse::<Ident>()?.to_string())
    }
}

/// Whether the next token is the identifier `word` (a contextual keyword like
/// `arity`, which is not a real Rust keyword).
fn peek_ident(input: ParseStream, word: &str) -> bool {
    input.fork().parse::<Ident>().is_ok_and(|id| id == word)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;
    use syn::parse::Parser;

    fn parse_module(ts: TokenStream) -> ClassSpec {
        ClassSpec::parse_module
            .parse2(ts)
            .expect("ruby_module! body should parse")
    }

    fn parse_class(ts: TokenStream) -> ClassSpec {
        ClassSpec::parse_class
            .parse2(ts)
            .expect("ruby_class! body should parse")
    }

    #[test]
    fn a_pure_mixin_module_with_operator_methods() {
        let spec = parse_module(quote! {
            Comparable = COMPARABLE_CLASS;
            def "<" arity 1 (recv, args, _block) { lt_impl(recv, args) }
            def "<=" arity 1 (recv, args, _block) { le_impl(recv, args) }
            def "between?" arity 2 (recv, args, _block) { between(recv, args) }
        });
        assert!(matches!(spec.kind, ClassKind::Module));
        assert_eq!(spec.name.to_string(), "Comparable");
        assert_eq!(spec.id.segments.last().unwrap().ident, "COMPARABLE_CLASS");
        assert_eq!(spec.methods.len(), 3);
        assert!(spec.methods.iter().all(|m| !m.is_class_method));
        assert_eq!(spec.methods[0].names[0].ruby, "<");
        assert_eq!(spec.methods[0].names[0].arity, Some(1));
        assert_eq!(spec.methods[2].names[0].ruby, "between?");
        assert_eq!(spec.methods[2].names[0].arity, Some(2));
    }

    #[test]
    fn a_bound_name_binds_a_callable_rust_fn_for_the_def() {
        let spec = parse_class(quote! {
            Set = SET_CLASS < OBJECT_CLASS;
            def "add" | "<<" as set_add (recv, args, _block) { do_add(recv, args) }
            def "union"(recv, args, _block) { set_add(recv, args) }
        });
        // The `as X` binds the FIRST def's fn name; aliases still share it.
        assert_eq!(
            spec.methods[0].bound_name.as_ref().unwrap().to_string(),
            "set_add"
        );
        assert_eq!(spec.methods[0].names.len(), 2);
        assert_eq!(spec.methods[0].names[1].ruby, "<<");
        // A def without `as` leaves the fn name to the mangler.
        assert!(spec.methods[1].bound_name.is_none());
    }

    #[test]
    fn a_class_with_superclass_constants_and_class_methods() {
        let spec = parse_class(quote! {
            Float = FLOAT_CLASS < NUMERIC_CLASS;
            include COMPARABLE_CLASS;
            const INFINITY = f64::INFINITY;
            const NAN = f64::NAN;
            def "nan?"(recv, _args, _block) { Ok(is_nan(recv)) }
            def self.pi(_recv, _args, _block) { Ok(RubyValue::Float(std::f64::consts::PI)) }
        });
        match &spec.kind {
            ClassKind::Class { superclass } => {
                let sup = superclass.as_ref().expect("Float declares a superclass");
                assert_eq!(sup.segments.last().unwrap().ident, "NUMERIC_CLASS");
            }
            ClassKind::Module => panic!("expected a class"),
        }
        assert_eq!(spec.includes.len(), 1);
        assert_eq!(
            spec.includes[0].segments.last().unwrap().ident,
            "COMPARABLE_CLASS"
        );
        assert_eq!(spec.consts.len(), 2);
        assert_eq!(spec.consts[0].name.to_string(), "INFINITY");
        assert_eq!(spec.methods.len(), 2);
        assert!(!spec.methods[0].is_class_method);
        assert!(spec.methods[1].is_class_method);
        assert_eq!(spec.methods[1].names[0].ruby, "pi");
    }

    /// A flag constant often exists on one platform only, so a `const` row
    /// carries its `#[cfg]` through to the installer the same way a `def` does.
    #[test]
    fn a_const_keeps_its_cfg_attribute() {
        let spec = parse_class(quote! {
            Fcntl = FCNTL_MODULE;
            const F_GETFL = flag(3);
            #[cfg(target_vendor = "apple")]
            const F_PREALLOCATE = flag(42);
        });
        assert_eq!(spec.consts.len(), 2);
        assert!(spec.consts[0].attrs.is_empty());
        assert_eq!(spec.consts[1].attrs.len(), 1);
        assert!(spec.consts[1].attrs[0].path().is_ident("cfg"));
    }

    #[test]
    fn the_root_class_may_omit_its_superclass() {
        // `BasicObject` is the root: its header has no `< SUPER`, so the parsed
        // superclass is `None` (every other class carries `Some`).
        let spec = parse_class(quote! {
            BasicObject = BASIC_OBJECT_CLASS;
            def "!"(recv, _args, _block) { Ok(negate(recv)) }
        });
        match &spec.kind {
            ClassKind::Class { superclass } => assert!(superclass.is_none()),
            ClassKind::Module => panic!("expected a class"),
        }
        assert_eq!(spec.methods.len(), 1);
        assert_eq!(spec.methods[0].names[0].ruby, "!");
    }

    #[test]
    fn shared_body_aliases_visibility_and_late_alias() {
        let spec = parse_module(quote! {
            Process = PROCESS_CLASS;
            def self."pid"(_recv, _args, _block) { pid() }
            def self."succ" | "next"(_recv, _args, _block) { succ() }
            private def helper(_recv, _args, _block) { helper_impl() }
            alias cmp = "<=>";
        });
        assert_eq!(spec.methods.len(), 3);
        // Shared-body aliases: two names, one def.
        assert_eq!(spec.methods[1].names.len(), 2);
        assert_eq!(spec.methods[1].names[0].ruby, "succ");
        assert_eq!(spec.methods[1].names[1].ruby, "next");
        // Visibility prefix.
        assert_eq!(spec.methods[2].visibility, Visibility::Private);
        assert_eq!(spec.methods[2].names[0].ruby, "helper");
        // Late alias.
        assert_eq!(spec.aliases.len(), 1);
        assert_eq!(spec.aliases[0].new_name, "cmp");
        assert_eq!(spec.aliases[0].old_name, "<=>");
    }

    #[test]
    fn nested_classes_namespace_under_the_outer_module() {
        let spec = parse_module(quote! {
            Process = PROCESS_CLASS;
            def self."pid"(_recv, _args, _block) { pid() }

            class Status = PROCESS_STATUS_CLASS < OBJECT_CLASS {
                def "exitstatus"(recv, _args, _block) { code(recv) }
                def "success?"(recv, _args, _block) { ok(recv) }
            }
            module Sub = SUB_MODULE {
                def "helper"(_recv, _args, _block) { Ok(RubyValue::Nil) }
            }
        });
        // The outer keeps its own members.
        assert_eq!(spec.methods.len(), 1);
        assert_eq!(spec.nested.len(), 2);
        // A nested class carries its own kind, id, superclass, and methods.
        let status = &spec.nested[0];
        assert_eq!(status.name.to_string(), "Status");
        assert_eq!(
            status.id.segments.last().unwrap().ident,
            "PROCESS_STATUS_CLASS"
        );
        match &status.kind {
            ClassKind::Class { superclass } => {
                let sup = superclass.as_ref().expect("Status declares a superclass");
                assert_eq!(sup.segments.last().unwrap().ident, "OBJECT_CLASS")
            }
            ClassKind::Module => panic!("Status should be a class"),
        }
        assert_eq!(status.methods.len(), 2);
        assert_eq!(status.methods[0].names[0].ruby, "exitstatus");
        // A nested `module` has no superclass.
        assert!(matches!(spec.nested[1].kind, ClassKind::Module));
        assert_eq!(spec.nested[1].name.to_string(), "Sub");
    }

    #[test]
    fn default_arity_is_none_meaning_variadic() {
        let spec = parse_module(quote! {
            M = MATH_CLASS;
            def "sqrt"(_recv, _args, _block) { Ok(RubyValue::Nil) }
        });
        assert_eq!(spec.methods[0].names[0].arity, None);
    }
}
