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
//!     receiver rstr = crate::RubyValue::Str;             // the unwrapped receiver
//!
//!     const INFINITY = f64::INFINITY;                    // 0+ constants (RHS is a Rust expr)
//!
//!     def "<=>"(recv, other) { /* real Rust */ }         // instance method (name: str or ident)
//!     def "succ" | "next" (recv) { .. }                  // aliases sharing one body
//!     private def helper(recv, *args) { .. }             // visibility prefix
//!     def self.pid(recv) { .. }                          // class/singleton method
//!     #[cfg(target_vendor = "apple")] def "change"(..){} // platform-gated def
//!
//!     alias cmp = "<=>";                                 // late alias (new = existing)
//!
//!     class Status = STATUS_CLASS < OBJECT_CLASS {        // nested, braced body
//!         def "exitstatus"(recv) { .. }                   //   -> Process::Status
//!     }
//! }
//! ```
//!
//! A def's PARAMETER LIST is the single declaration of its shape. Both the
//! argument-count guard the macro emits and the number `Method#arity` reports
//! come from it, so the two cannot disagree:
//!
//! ```text
//! (recv)                        // 0        exactly none
//! (recv, needle)                // 1        exactly one
//! (recv, needle, start = nil)   // -2       one required, one defaulted
//! (recv, from, to?)             // -2       `?` binds Option, to tell absent from nil
//! (recv, *items)                // -1       any number
//! (recv, at, *rest)             // -2       one or more -- "expected 1+"
//! (recv, fmt, **opts)           // -2       kwargs count as one extra slot
//! (recv, &block)                // 0        a block never counts
//! ```
//!
//! The number follows CRuby's own equation (`proc.c:1655`, `proc.c:3452`):
//! `min` and `max` from the signature, then `(min == max) ? min : -min-1`.
//! CRuby applies it to C methods too, but C declares only `argc = N` or
//! `argc = -1` -- it cannot say "one required plus one optional", which is why
//! `String#index` reports -1 and why no C method reports below -1. A `cfunc`
//! marker before the parameter list records that lost precision and collapses a
//! ranged signature back to the -1 CRuby reports. It is one bit, and the only
//! arity fact left for a human to write:
//!
//! ```text
//! def "index" cfunc (recv, needle, start = nil) { .. }   // -2 by the equation, -1 in CRuby
//! ```
//!
//! A per-name `arity N` override remains for the rare def whose `|`-joined
//! names genuinely differ (`"<<"` takes exactly one where `push` is variadic).
//!
//! One caveat worth knowing: the guard is per-DEF (one shared body) while the
//! reported arity is per-NAME, so a def whose names disagree cannot raise
//! differently for each. That was equally true of the hand-written guards this
//! replaced.
//!
//! A class may declare `receiver NAME = VARIANT;`. Its table is keyed by
//! `ClassId`, so a row's receiver is ALWAYS that `RubyValue` variant -- and
//! unwrapping it was 234 identical `recv_str!(recv)` calls across String, Array
//! and Hash. The header says it once and every body may name `NAME` directly;
//! the untyped receiver slot is still there for the rows that need it.
//!
//! A body may also read `__args`, the full argument slice, for the few rows
//! that forward their arguments on verbatim -- an `Enumerator` that re-invokes
//! the method it came from, or a delegator that hands the list to another
//! object. The parameter list still declares the shape; `__args` only avoids
//! rebuilding a slice the caller already passed.
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
    /// `receiver NAME = VARIANT;` -- the unwrapped receiver every def in this
    /// class may name directly. A table row is keyed by `ClassId`, so its
    /// receiver is ALWAYS the matching `RubyValue` variant; unwrapping it was
    /// 234 identical `recv_str!(recv)` calls before this. Bodies that need the
    /// untyped value still have the receiver slot itself.
    pub receiver: Option<(Ident, Path)>,
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
    /// The receiver slot, spelled by the author (`recv`/`_recv`). Not a Ruby
    /// parameter -- Ruby's own `def` does not list `self` either.
    pub recv: Ident,
    /// Positional parameters in source order: required first, then optional.
    pub params: Vec<Param>,
    /// `*rest` -- makes the accepted count unbounded.
    pub rest: Option<Ident>,
    /// `**kwrest` -- the trailing options Hash, which CRuby counts as exactly
    /// one extra positional slot.
    pub kwrest: Option<Ident>,
    /// `&block`. A block never affects arity.
    pub block: Option<Ident>,
    /// CRuby implements this method as a C function that threw its signature
    /// away (`rb_define_method(..., -1)` plus `rb_scan_args` inside). See
    /// [`MethodDef::derived_arity`].
    pub cfunc: bool,
    /// The `{ ... }` body -- real Rust, kept verbatim for the proc-macro.
    pub body: TokenStream,
}

/// One positional parameter of a `def`.
pub struct Param {
    pub name: Ident,
    pub kind: ParamKind,
}

/// Required, or optional in one of two flavours.
pub enum ParamKind {
    /// `x` -- binds `&RubyValue`.
    Required,
    /// `x = EXPR` -- binds `&RubyValue`; the default is evaluated ONLY when the
    /// caller omitted the argument. Boxed because a `syn::Expr` is 240 bytes
    /// and the other variants carry nothing.
    Optional(Box<Expr>),
    /// `x?` -- binds `Option<&RubyValue>`, for the methods that must tell an
    /// absent argument from an explicit `nil`.
    Maybe,
}

impl MethodDef {
    /// The fewest arguments this method accepts.
    pub fn min_args(&self) -> usize {
        self.params
            .iter()
            .filter(|p| matches!(p.kind, ParamKind::Required))
            .count()
    }

    /// The most it accepts; `None` when a `*rest` makes that unbounded.
    pub fn max_args(&self) -> Option<usize> {
        if self.rest.is_some() {
            return None;
        }
        Some(self.params.len() + usize::from(self.kwrest.is_some()))
    }

    /// What `Method#arity` reports, by CRuby's own equation
    /// (`proc.c:1655`, `proc.c:3452`):
    ///
    /// ```text
    /// arity = (min == max) ? min : -min-1
    /// ```
    ///
    /// CRuby applies this to C methods too, but a C function declares only an
    /// argument COUNT, so `min` and `max` are either both `N` or `0` and
    /// unbounded. It cannot say "1 required plus 1 optional".
    ///
    /// `cfunc` marks a method CRuby declares `argc = -1` and then checks by
    /// hand with `rb_scan_args`. Such a method reports -1 whatever its real
    /// shape -- `Dir#entries` accepts exactly zero arguments and still reports
    /// -1 -- so the marker overrides the equation rather than refining it. This
    /// is the one arity fact the signature cannot supply, and it is a bit
    /// rather than a number.
    pub fn derived_arity(&self) -> i64 {
        if self.cfunc {
            return -1;
        }
        let min = self.min_args();
        match self.max_args() {
            Some(max) if max == min => min as i64,
            _ => -(min as i64) - 1,
        }
    }
}

/// One Ruby method name plus an explicit `Method#arity` override. Normally
/// `None`: the number comes from the parameter list. It exists for a def whose
/// `|`-joined names genuinely differ (`"<<"` is 1 where `push` is variadic).
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
            receiver: None,
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
        "receiver" => {
            if !attrs.is_empty() {
                return Err(attrs_forbidden(input));
            }
            input.parse::<Ident>()?; // `receiver`
            let name: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            let variant = Path::parse_mod_style(input)?;
            input.parse::<Token![;]>()?;
            if spec.receiver.is_some() {
                return Err(syn::Error::new(
                    name.span(),
                    "a class declares at most one `receiver`",
                ));
            }
            spec.receiver = Some((name, variant));
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

    // `cfunc`: CRuby declares this one `argc = -1`, so it reports -1 whatever
    // the parameter list says. Per-def, and always immediately before the
    // parameters it qualifies.
    let cfunc = if peek_ident(input, "cfunc") {
        input.parse::<Ident>()?;
        true
    } else {
        false
    };

    let buf;
    parenthesized!(buf in input);
    let recv: Ident = buf.parse()?;
    let (params, rest, kwrest, block) = parse_params(&buf)?;

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
        params,
        rest,
        kwrest,
        block,
        cfunc,
        body,
    })
}

/// Parse the parameters after the receiver slot:
/// `required* optional* [*rest] [**kwrest] [&block]`.
///
/// Order is enforced here rather than left to the reader, because the order is
/// what makes the argument-count guard and the reported arity derivable at all.
/// Post-required parameters (`(a, *r, b)`) are rejected: no builtin needs them,
/// and they would make the guard a two-sided split for no gain.
#[allow(clippy::type_complexity)]
fn parse_params(
    input: ParseStream,
) -> syn::Result<(Vec<Param>, Option<Ident>, Option<Ident>, Option<Ident>)> {
    let mut params: Vec<Param> = Vec::new();
    let mut rest = None;
    let mut kwrest = None;
    let mut block = None;

    while input.peek(Token![,]) {
        input.parse::<Token![,]>()?;
        if input.is_empty() {
            break;
        }

        if input.peek(Token![&]) {
            input.parse::<Token![&]>()?;
            let name: Ident = input.parse()?;
            if block.replace(name).is_some() {
                return Err(input.error("a def takes at most one `&block`"));
            }
            continue;
        }
        if input.peek(Token![*]) && input.peek2(Token![*]) {
            input.parse::<Token![*]>()?;
            input.parse::<Token![*]>()?;
            let name: Ident = input.parse()?;
            if kwrest.replace(name).is_some() {
                return Err(input.error("a def takes at most one `**kwrest`"));
            }
            continue;
        }
        if input.peek(Token![*]) {
            input.parse::<Token![*]>()?;
            let name: Ident = input.parse()?;
            if rest.replace(name).is_some() {
                return Err(input.error("a def takes at most one `*rest`"));
            }
            continue;
        }

        if block.is_some() || kwrest.is_some() || rest.is_some() {
            return Err(input
                .error("positional parameters must come before `*rest`, `**kwrest` and `&block`"));
        }

        let name: Ident = input.parse()?;
        let kind = if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;
            ParamKind::Optional(Box::new(input.parse()?))
        } else if input.peek(Token![?]) {
            input.parse::<Token![?]>()?;
            ParamKind::Maybe
        } else {
            if params
                .iter()
                .any(|p| !matches!(p.kind, ParamKind::Required))
            {
                return Err(syn::Error::new(
                    name.span(),
                    "a required parameter cannot follow an optional one",
                ));
            }
            ParamKind::Required
        };
        params.push(Param { name, kind });
    }

    Ok((params, rest, kwrest, block))
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

    /// Two bare parameters mean two required arguments. They used to be read as
    /// the legacy `(recv, args, block)` triple, which is why every header was
    /// swept to the explicit `(recv, *args, &block)` before that branch went.
    #[test]
    fn two_bare_parameters_are_two_required_arguments() {
        let spec = parse_module(quote! {
            M = M_CLASS;
            def "insert"(_recv, at, other) { }
        });
        let m = &spec.methods[0];
        assert_eq!(m.params.len(), 2);
        assert!(m.rest.is_none() && m.block.is_none());
        assert_eq!(m.derived_arity(), 2);
    }

    /// CRuby's `arity = (min == max) ? min : -min-1`, over every shape the
    /// grammar can express. `cfunc` is the one bit a human writes, and it only
    /// matters when min != max.
    #[test]
    fn arity_follows_crubys_equation() {
        let spec = parse_module(quote! {
            M = MATH_CLASS;
            def "length"(_recv) { }
            def "index"(_recv, needle) { }
            def "insert"(_recv, at, other) { }
            def "first"(_recv, n = RubyValue::Nil) { }
            def "slice"(_recv, from, to?) { }
            def "push"(_recv, *items) { }
            def "unshift"(_recv, at, *rest) { }
            def "unpack"(_recv, fmt, **opts) { }
            def "each"(_recv, &blk) { }
            def "sub" cfunc (_recv, pattern, replacement?) { }
            def "kill" cfunc (_recv, sig, *pids) { }
            def "entries" cfunc (_recv) { }
        });
        let arity = |i: usize| spec.methods[i].derived_arity();
        assert_eq!(arity(0), 0, "()");
        assert_eq!(arity(1), 1, "(a)");
        assert_eq!(arity(2), 2, "(a, b)");
        assert_eq!(arity(3), -1, "(a = default) is min 0, max 1");
        assert_eq!(arity(4), -2, "(a, b?) is min 1, max 2");
        assert_eq!(arity(5), -1, "(*rest) is min 0, unbounded");
        assert_eq!(arity(6), -2, "(a, *rest) is min 1, unbounded");
        assert_eq!(arity(7), -2, "kwargs count as one extra slot: min 1, max 2");
        assert_eq!(arity(8), 0, "a block never counts");
        assert_eq!(
            arity(9),
            -1,
            "cfunc collapses the range C could not express"
        );
        assert_eq!(arity(10), -1, "cfunc collapses the unbounded case too");
        assert_eq!(
            arity(11),
            -1,
            "a -1 cfunc reports -1 even when it accepts exactly none"
        );
    }

    /// A `|`-joined def whose names genuinely differ keeps the override, and it
    /// wins over the derived value.
    #[test]
    fn an_explicit_arity_overrides_the_signature() {
        let spec = parse_module(quote! {
            M = ARRAY_CLASS;
            def "<<" arity 1 | "push" | "append"(_recv, *items) { }
        });
        let m = &spec.methods[0];
        assert_eq!(m.derived_arity(), -1);
        assert_eq!(m.names[0].arity, Some(1));
        assert_eq!(m.names[1].arity, None);
    }

    #[test]
    fn parameters_must_be_ordered() {
        let bad = [
            quote! { M = M_CLASS; def "x"(_recv, *rest, tail) { } },
            quote! { M = M_CLASS; def "x"(_recv, a = RubyValue::Nil, b) { } },
            quote! { M = M_CLASS; def "x"(_recv, &blk, a) { } },
        ];
        for ts in bad {
            assert!(
                ClassSpec::parse_module.parse2(ts).is_err(),
                "an out-of-order parameter list must be rejected"
            );
        }
    }
}
