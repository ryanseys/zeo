//! The shared grammar for the `ruby_class! { ... }` DSL.
//!
//! One Ruby core class/module per file (plus any it namespaces via nested
//! `class`/`module` items), declared in a Ruby-like syntax whose method bodies
//! stay real Rust. Parsed here with `syn` so one grammar backs both consumers
//! and they cannot drift:
//!
//! - `zeo-macros`' `ruby_class!` proc-macro emits the runtime code.
//! - `zeo`'s build.rs re-parses the same invocations out of the runtime source
//!   and projects `CLASS_SURFACE` -- the shape and method/constant NAMES the
//!   compiler folds `respond_to?`/`is_a?`/const lookups against. It reads only
//!   the headers; method bodies are opaque.
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
//! (recv, n?, random:?)          // -1       a NAMED keyword, bound from the
//!                               //          same trailing Hash `**opts` peels
//! (recv, sep = nil, chomp:)     // -2       `k:` is required -- absent raises
//! (recv, &block)                // 0        a block never counts
//! ```
//!
//! A keyword binds OWNED (`RubyValue` / `Option<RubyValue>`) where a positional
//! binds borrowed, and the asymmetry is forced rather than sloppy: a positional
//! is an element of the caller's argv, alive for the whole call, while a
//! keyword lives inside a Hash whose lock must not be held across body code.
//! The clone is an `Arc` bump for a heap value and nothing for an immediate.
//! A def that declares named keywords and NO `**kwrest` refuses an undeclared
//! key, as ruby does.
//!
//! `ruby def` marks a list as ruby's OWN signature, so `Method#parameters`
//! reports its names and kinds instead of the anonymous descriptor derived from
//! the arity -- and the arity then falls out of the same list, so the two
//! answers cannot disagree:
//!
//! ```text
//! ruby def "sample"(recv, n?, random:?) { .. }   // [[:opt, :n], [:key, :random]], -1
//! ```
//!
//! It is opt-in per def because it has to be: ruby reports NO parameter name on
//! 1,025 of its own builtin rows, and 793 more have a body shape that differs
//! from the signature on purpose (they take `*args` and peel). Deriving names
//! from every list would invent the first group and misreport the second. An
//! unmarked def is byte-identical to before. Where the two genuinely differ, a
//! per-name `params "..."` states ruby's answer directly.
//!
//! The number follows CRuby's equation in `proc.c`: `min` and `max` from the
//! signature, then `(min == max) ? min : -min-1`. CRuby applies it to C methods
//! too, but C declares only `argc = N` or `argc = -1` -- it cannot say "one
//! required plus one optional", which is why `String#index` reports -1 and why
//! no C method reports below -1. A `cfunc` marker records that lost precision
//! and collapses a ranged signature back to -1:
//!
//! ```text
//! def "index" cfunc (recv, needle, start = nil) { .. }   // -2 by the equation, -1 in CRuby
//! ```
//!
//! A per-name `arity N` override remains for the rare def whose `|`-joined
//! names genuinely differ (`"<<"` takes exactly one where `push` is variadic).
//! The guard is per-DEF (one shared body) while the reported arity is per-NAME,
//! so such a def cannot raise differently for each name.
//!
//! A class may declare `receiver NAME = VARIANT;`. Its table is keyed by
//! `ClassId`, so a row's receiver is ALWAYS that `RubyValue` variant; the
//! header unwraps it once and every body may name `NAME` directly. The untyped
//! receiver slot remains for rows that need it.
//!
//! A body may also read `__args`, the full argument slice, for rows that
//! forward their arguments verbatim. The parameter list still declares the
//! shape; `__args` only avoids rebuilding a slice the caller already passed.
//!
//! Superclass and `include` targets are written as `ClassId` CONSTS (the one
//! hard-ABI token), not names -- so the build.rs projection can emit them
//! symbolically (`zeo_abi::OBJECT_CLASS`) and let rustc resolve them, never
//! evaluating a const itself. Only the header NAME is a plain identifier.

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
    /// `seed <path>;` -- a fn the class's `install_constants` thunk calls.
    ///
    /// For a constant set a `const` row cannot express: one computed name, a
    /// loop over a table, a value that also seeds a global. It runs where a
    /// `const` row runs, so it is paid only by a program that ships this
    /// class's table -- which is the whole point of moving a bootstrap seeder
    /// here.
    pub seeds: Vec<Path>,
    /// `allocate <path>;` -- a fn answering a blank instance of this class,
    /// for `Class#allocate` and for the `allocate`-then-`initialize` half of
    /// `Class#new`.
    ///
    /// Only a class whose CONSTRUCTOR CARRIES STATE needs one. Without it
    /// `Class#new` calls the registered constructor, which is right until a
    /// program reopens the class with its own `initialize` -- ruby's `new` is
    /// `allocate` plus `initialize`, and a fused constructor cannot run a body
    /// it does not know about.
    pub allocate: Option<Path>,
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
    /// Named keyword parameters (`k:`, `k: EXPR`, `k:?`), taken out of the
    /// same trailing Hash `kwrest` peels. Declared AFTER `*rest` and before
    /// `**kwrest`, which is ruby's own order.
    pub keywords: Vec<KwParam>,
    /// `ruby def` -- this parameter list is ruby's OWN signature, so
    /// `Method#parameters` reports its names and kinds rather than the
    /// anonymous descriptor derived from the arity.
    ///
    /// Opt-in per def, and it has to be: ruby reports NO parameter name for
    /// 1,025 of its own builtin rows, so deriving names from every list would
    /// invent that many. An unmarked def is byte-identical to before.
    pub ruby_sig: bool,
    /// `**kwrest` -- the trailing options Hash, which CRuby counts as exactly
    /// one extra positional slot.
    pub kwrest: Option<Ident>,
    /// `**kwrest!` -- peel only a Hash the CALLER marked as keywords.
    ///
    /// The plain `**kwrest` takes ANY trailing Hash, which is right for a row
    /// whose options CRuby reads with `rb_scan_args`' old-style `:`. A row
    /// reading them with `rb_scan_args_kw` and the caller's own semantics does
    /// not: `IO.new(fd, {external_encoding: "x"})` hands the MODE slot a Hash
    /// and is a TypeError, while `IO.new(fd, external_encoding: "x")` is
    /// options. Only the mark tells the two apart.
    pub kwrest_strict: bool,
    /// `&block`. A block never affects arity.
    pub block: Option<Ident>,
    /// CRuby implements this method as a C function that threw its signature
    /// away (`rb_define_method(..., -1)` plus `rb_scan_args` inside). See
    /// [`MethodDef::derived_arity`].
    pub cfunc: bool,
    /// `allocs`: this CLASS method allocates through the RECEIVER class, so a
    /// subclass receiver gets an instance of itself.
    ///
    /// CRuby hands every class-method C function the real receiver as `klass`
    /// and lets the function decide, which is why no list of these exists
    /// there -- the fact lives at each definition. Three behaviours, all in
    /// CRuby today: `rb_ary_s_create` threads it (`ary_new(klass, argc)`);
    /// `enumerator_s_produce` ignores it (`rb_enumeratorize_with_size_kw`
    /// pins `base_class = rb_cEnumerator`); `thread_s_current` ignores it and
    /// hands back an object that already exists. zeo's value-builtin payloads
    /// carry no class id of their own, so the wrapper outside the row has to
    /// know -- and this marker is how the row tells it, at the definition,
    /// where CRuby keeps the same knowledge.
    ///
    /// Defaults to false, i.e. "answers the base class". That is the safe
    /// direction: an unmarked new row demotes rather than inventing a
    /// subclass instance.
    pub allocs: bool,
    /// `inherits`: this row exists so DISPATCH finds it here, but ruby owns
    /// the method further up the ancestry, so reflection must attribute it
    /// there.
    ///
    /// The inverse of `own_row!`/`inherited_row!`, which exist for the
    /// opposite case -- ruby owning a method on the SUBCLASS while the body
    /// lives on an ancestor. Here the body has to live on the subclass (it is
    /// the only table a receiver of that shape reaches, or the only one that
    /// can see the payload), while ruby reports an ancestor as the owner:
    /// `Module#instance_variable_get` is really `Kernel`'s, and `Hash.new` is
    /// really `Class#new`.
    ///
    /// A marked row is skipped by `instance_methods(false)` /
    /// `singleton_methods(false)` / `private_instance_methods(false)`, and the
    /// owner scan walks past it to the ancestor that really declares it -- so
    /// the ancestor MUST have a row of that name, or `.owner` answers nil.
    ///
    /// Defaults to false, i.e. "this class owns it". That is the safe
    /// direction: an unmarked new row claims ownership rather than silently
    /// disappearing from the class's own surface.
    pub inherits: bool,
    /// `hidden`: the row exists for DISPATCH only -- ruby has no such method,
    /// so every listing and every `respond_to?`/`method_defined?` must answer
    /// as if it were absent. It covers a helper zeo's own generated code
    /// calls (`Struct`'s initialize bridge) and a row zeo keeps to raise
    /// ruby's error where ruby reaches a method of its own.
    ///
    /// Defaults to false: an unmarked row is a method ruby has.
    pub hidden: bool,
    /// `gated "io/console"` / `gated env "boxes"`: ruby only grows this row
    /// when the named feature is required (or the named switch is on), so
    /// the runtime hides it until then -- `respond_to?(:getch)` is how a
    /// library asks whether the console extension is there at all. The
    /// macro bakes the marker into the table's `gate` fn; the runtime's
    /// `builtins::gate` maps each name to the switch that arms it. `None`
    /// -- the default -- is an always-on row.
    pub gate: Option<Gate>,
    /// The `{ ... }` body -- real Rust, kept verbatim for the proc-macro.
    pub body: TokenStream,
}

/// A [`MethodDef::gate`]'s payload: the switch that reveals the row.
pub struct Gate {
    /// `gated env "..."` -- armed by a process-wide switch read at startup
    /// rather than by a `require`.
    pub env: bool,
    /// The feature (`"io/console"`) or switch (`"boxes"`) name.
    pub feature: String,
}

impl Gate {
    /// The one string the generated table carries: the feature name, with
    /// the env-armed kind prefixed `env:` so the runtime's gate map can
    /// tell the two arming kinds apart without a second column.
    pub fn key(&self) -> String {
        if self.env {
            format!("env:{}", self.feature)
        } else {
            self.feature.clone()
        }
    }
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

/// How a KEYWORD parameter is declared. The three mirror the positional kinds
/// and answer the same question ruby's `#parameters` does.
pub enum KwKind {
    /// `k:` -- required; an absent key raises `ArgumentError`. Binds
    /// `RubyValue`.
    Required,
    /// `k: EXPR` -- the default is evaluated only when the key is absent.
    /// Binds `RubyValue`.
    Optional(Box<Expr>),
    /// `k:?` -- binds `Option<RubyValue>`, for a body that must tell an absent
    /// keyword from an explicit `nil`. The common spelling, and the one every
    /// hand-written peel this replaces already returned.
    Maybe,
}

/// A DSL ident as ruby spells it: a raw ident (`r#in`, for a ruby name that
/// collides with a Rust keyword) drops the `r#`.
pub fn ruby_ident(i: &Ident) -> String {
    let s = i.to_string();
    s.strip_prefix("r#").map_or(s.clone(), str::to_owned)
}

/// One named keyword parameter.
///
/// OWNED, where a positional is borrowed, and the asymmetry is forced rather
/// than sloppy: a positional is an element of the `&[RubyValue]` argv the
/// caller owns for the whole call, so a borrow is free and safe, while a
/// keyword lives INSIDE a Hash behind an `Arc<Mutex<..>>` whose guard this
/// runtime forbids holding across body code (the re-entry rule `Freezable`
/// documents). So the value is cloned out: an `Arc` bump for a heap value,
/// nothing for an immediate.
pub struct KwParam {
    pub name: Ident,
    pub kind: KwKind,
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

    /// The descriptor this parameter list implies, in ruby's canonical order --
    /// what `Method#parameters` reports for a `ruby def`.
    ///
    /// `None` for a def not marked `ruby`, whose list describes how the BODY
    /// receives its arguments rather than what ruby reports. The two are
    /// genuinely different for a native row: 793 of them take `*args` and
    /// peel, where ruby reports a real signature.
    pub fn derived_params(&self) -> Option<Vec<SigParam>> {
        if !self.ruby_sig {
            return None;
        }
        // A RAW ident (`r#in`) spells a ruby name that is a Rust keyword;
        // the `r#` is Rust's escape and never part of what ruby calls it.
        let named = |i: &Ident| Some(ruby_ident(i));
        let mut out: Vec<SigParam> = self
            .params
            .iter()
            .map(|p| SigParam {
                kind: match p.kind {
                    ParamKind::Required => SigKind::Req,
                    ParamKind::Optional(_) | ParamKind::Maybe => SigKind::Opt,
                },
                name: named(&p.name),
            })
            .collect();
        if let Some(r) = &self.rest {
            out.push(SigParam {
                kind: SigKind::Rest,
                name: named(r),
            });
        }
        out.extend(self.keywords.iter().map(|k| SigParam {
            kind: match k.kind {
                KwKind::Required => SigKind::KeyReq,
                KwKind::Optional(_) | KwKind::Maybe => SigKind::Key,
            },
            name: named(&k.name),
        }));
        if let Some(k) = &self.kwrest {
            out.push(SigParam {
                kind: SigKind::KeyRest,
                name: named(k),
            });
        }
        if let Some(b) = &self.block {
            out.push(SigParam {
                kind: SigKind::Block,
                name: named(b),
            });
        }
        Some(out)
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
        // A `ruby def` derives its arity from the signature it declares, so
        // `#arity` and `#parameters` cannot disagree -- the same equation
        // CRuby applies, where any REQUIRED keyword adds one mandatory slot
        // and an optional one only makes the method variadic.
        if let Some(sig) = self.derived_params() {
            return signature_arity(&sig);
        }
        let min = self.min_args();
        match self.max_args() {
            Some(max) if max == min => min as i64,
            _ => -(min as i64) - 1,
        }
    }
}

/// One parameter kind, exactly as `Method#parameters` reports it. Mirrored
/// here rather than shared with the runtime because the dependency runs the
/// other way: `zeo-rt` reads this crate through the proc-macro, never back.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SigKind {
    Req,
    Opt,
    Rest,
    KeyReq,
    Key,
    KeyRest,
    Block,
}

impl SigKind {
    /// The runtime `ParamKind` variant this maps to, as a bare ident the
    /// proc-macro pastes under `crate::method_meta::ParamKind`.
    pub fn variant(self) -> &'static str {
        match self {
            SigKind::Req => "Req",
            SigKind::Opt => "Opt",
            SigKind::Rest => "Rest",
            SigKind::KeyReq => "KeyReq",
            SigKind::Key => "Key",
            SigKind::KeyRest => "KeyRest",
            SigKind::Block => "Block",
        }
    }
}

/// One entry of a `params "..."` spelling.
///
/// A bare `*`/`**`/`&` is named for its own SIGIL, not left nameless: that is
/// what ruby reports for an anonymous forwarding slot
/// (`Ractor#send.parameters` is `[[:rest, :*], [:keyrest, :**], [:block, :&]]`).
/// A genuinely nameless slot needs no spelling at all -- it is what the
/// arity-derived anonymous descriptor already produces -- so `name` is never
/// `None` here today, and stays an Option only because the descriptor it
/// becomes has one.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SigParam {
    pub kind: SigKind,
    pub name: Option<String>,
}

/// Split a signature at its TOP-LEVEL commas. A default value is arbitrary
/// Ruby (`opt = [1, 2]`, `enc = Encoding::UTF_8`), so the split tracks bracket
/// depth and string quoting rather than counting commas.
fn split_params(spec: &str) -> Vec<String> {
    let (mut out, mut cur, mut depth) = (Vec::new(), String::new(), 0i32);
    let mut quote: Option<char> = None;
    for c in spec.chars() {
        if let Some(q) = quote {
            cur.push(c);
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => {
                quote = Some(c);
                cur.push(c);
            }
            '(' | '[' | '{' => {
                depth += 1;
                cur.push(c);
            }
            ')' | ']' | '}' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// Parse a `params "..."` spelling -- ruby's own `def` signature without the
/// parentheses -- into what `Method#parameters` answers.
///
/// This is metadata, not a binding: the def's Rust parameter list says how the
/// BODY receives its arguments, and this says what ruby REPORTS. They are
/// different questions for a native row, which is why the DSL could not spell
/// a keyword at all before -- a keyword arrives inside the options Hash the
/// body already takes as one positional slot.
///
/// The default value after `=` or `key:` is read only to tell an optional
/// parameter from a required one; nothing keeps it, because
/// `Method#parameters` does not report defaults either.
pub fn parse_signature(spec: &str) -> Result<Vec<SigParam>, String> {
    let mut out = Vec::new();
    for raw in split_params(spec) {
        let p = raw.trim();
        if p.is_empty() {
            return Err(format!("empty parameter in params \"{spec}\""));
        }
        // A bare sigil keeps the sigil as its name -- ruby's own answer.
        let named = |rest: &str, sigil: &str| -> Option<String> {
            let r = rest.trim();
            Some(if r.is_empty() {
                sigil.to_string()
            } else {
                r.to_string()
            })
        };
        let entry = if let Some(rest) = p.strip_prefix("**") {
            SigParam {
                kind: SigKind::KeyRest,
                name: named(rest, "**"),
            }
        } else if let Some(rest) = p.strip_prefix('*') {
            SigParam {
                kind: SigKind::Rest,
                name: named(rest, "*"),
            }
        } else if let Some(rest) = p.strip_prefix('&') {
            SigParam {
                kind: SigKind::Block,
                name: named(rest, "&"),
            }
        } else if let Some((head, tail)) = split_keyword(p) {
            // `name:` is required, `name: default` is optional -- ruby's own
            // rule, and the only thing the default text is read for.
            let kind = if tail.trim().is_empty() {
                SigKind::KeyReq
            } else {
                SigKind::Key
            };
            SigParam {
                kind,
                name: Some(head.to_string()),
            }
        } else if let Some((head, _)) = p.split_once('=') {
            SigParam {
                kind: SigKind::Opt,
                name: Some(head.trim().to_string()),
            }
        } else {
            SigParam {
                kind: SigKind::Req,
                name: Some(p.to_string()),
            }
        };
        let sigil = matches!(entry.name.as_deref(), Some("*" | "**" | "&"));
        if !sigil && entry.name.as_deref().is_some_and(|n| !is_param_name(n)) {
            return Err(format!(
                "'{p}' is not a parameter spelling (in params \"{spec}\")"
            ));
        }
        out.push(entry);
    }
    Ok(out)
}

/// `name:` / `name: default`, told apart from a `::` scope inside a default
/// (`enc = Encoding::UTF_8` must not read as a keyword named `enc = Encoding`).
fn split_keyword(p: &str) -> Option<(&str, &str)> {
    let bytes = p.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b':' {
            if bytes.get(i + 1) == Some(&b':') {
                return None;
            }
            let (head, tail) = p.split_at(i);
            return is_param_name(head.trim()).then_some((head.trim(), &tail[1..]));
        }
        if bytes[i] == b'=' {
            return None;
        }
        i += 1;
    }
    None
}

fn is_param_name(n: &str) -> bool {
    !n.is_empty()
        && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !n.starts_with(|c: char| c.is_ascii_digit())
}

/// CRuby's signed arity for a `params` spelling -- the same equation
/// `zeo_rt::method_meta::arity_of` applies to a compiled method's descriptor,
/// so a row that spells its parameters needs no `arity N` override and the two
/// answers cannot drift.
pub fn signature_arity(d: &[SigParam]) -> i64 {
    let mut mandatory = d.iter().filter(|p| p.kind == SigKind::Req).count() as i64;
    let mut variadic = d
        .iter()
        .any(|p| matches!(p.kind, SigKind::Opt | SigKind::Rest));
    if d.iter().any(|p| p.kind == SigKind::KeyReq) {
        mandatory += 1;
    } else if d
        .iter()
        .any(|p| matches!(p.kind, SigKind::Key | SigKind::KeyRest))
    {
        variadic = true;
    }
    if variadic { -mandatory - 1 } else { mandatory }
}

/// One Ruby method name plus an explicit `Method#arity` override. Normally
/// `None`: the number comes from the parameter list. It exists for a def whose
/// `|`-joined names genuinely differ (`"<<"` is 1 where `push` is variadic).
pub struct MethodName {
    pub ruby: String,
    pub arity: Option<i64>,
    /// A `params "path, mode = nil, opt: nil"` spelling: what
    /// `Method#parameters` reports for this name. `None` -- the default, and
    /// most rows -- falls back to the anonymous descriptor the runtime derives
    /// from the arity, which names no parameter at all.
    ///
    /// Per-NAME for the same reason `arity` is: `|`-joined names share a body
    /// but not a signature.
    pub params: Option<Vec<SigParam>>,
    /// Per-NAME [`MethodDef::inherits`], for a def whose `|`-joined names
    /// differ in ownership: ruby reaches `Regexp.new` through `Class#new` but
    /// declares `Regexp.compile` on Regexp itself, and both share one body.
    /// ORed with the def-wide marker, so either spelling works.
    pub inherits: bool,
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
            seeds: Vec::new(),
            allocate: None,
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
            "expected `include`, `seed`, `allocate`, `const`, `alias`, `private`, `protected`, `module_function`, `def`, `class`, or `module`",
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
        "seed" => {
            if !attrs.is_empty() {
                return Err(attrs_forbidden(input));
            }
            input.parse::<Ident>()?; // `seed`
            spec.seeds.push(Path::parse_mod_style(input)?);
            input.parse::<Token![;]>()?;
        }
        "allocate" => {
            if !attrs.is_empty() {
                return Err(attrs_forbidden(input));
            }
            input.parse::<Ident>()?; // `allocate`
            if spec.allocate.is_some() {
                return Err(input.error("a class declares at most one `allocate`"));
            }
            spec.allocate = Some(Path::parse_mod_style(input)?);
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
        // `def` and `ruby def` reach the same parser; `parse_def` consumes the
        // marker itself, so the two arms differ only in what the dispatcher
        // had to look at to get here.
        "def" | "ruby" => {
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
    // `ruby def` -- the parameter list that follows is ruby's OWN signature,
    // so its names and kinds are what `Method#parameters` reports. Written
    // here rather than inferred, because ruby names no parameter at all on
    // 1,025 of its builtin rows and inferring would invent that many.
    let ruby_sig = peek_ident(input, "ruby");
    if ruby_sig {
        input.parse::<Ident>()?; // `ruby`
    }
    input.parse::<Ident>()?; // `def`

    // `self .` marks a class/singleton method.
    let is_class_method = input.peek(Token![self]) && input.peek2(Token![.]);
    if is_class_method {
        input.parse::<Token![self]>()?;
        input.parse::<Token![.]>()?;
    }

    // One or more `NAME [arity N] [inherits]`, separated by `|`, sharing one
    // body. Both qualifiers are per-NAME because ruby's answers are: two
    // `|`-joined names can differ in reported arity AND in owner.
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
        // `params "..."` -- ruby's own signature for this name. Written after
        // `arity` when both appear, though a spelling makes the override
        // redundant: the arity falls out of it (`signature_arity`).
        let params = if peek_ident(input, "params") {
            input.parse::<Ident>()?; // `params`
            let lit: syn::LitStr = input.parse()?;
            let spec = lit.value();
            Some(parse_signature(&spec).map_err(|e| syn::Error::new(lit.span(), e))?)
        } else {
            None
        };
        // A per-NAME `inherits` is recognized only when another `|`-joined
        // name follows it. A TRAILING one would be indistinguishable from the
        // def-wide marker parsed below, and silently qualifying just the last
        // name is exactly the misreading that would cause -- so the trailing
        // spelling always means "the whole def", which is the common case.
        let inherits = peek_ident(input, "inherits") && input.peek2(Token![|]);
        if inherits {
            input.parse::<Ident>()?;
        }
        names.push(MethodName {
            ruby,
            arity,
            params,
            inherits,
        });
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
    // ...and `allocs`: allocates through the receiver class (see
    // `MethodDef::allocs`). Both qualify the def that follows, and either
    // order reads fine, so neither is positional -- writing them the other way
    // round must not become a parse error at the def, far from any explanation.
    // ...and `inherits`: ruby owns this one further up the ancestry, so
    // reflection must not report this class (see `MethodDef::inherits`).
    // ...and `gated "feature"` / `gated env "switch"`: the row only exists
    // once its feature is required / its switch is on (see
    // `MethodDef::gate`).
    let (mut cfunc, mut allocs, mut inherits) = (false, false, false);
    let mut hidden = false;
    let mut gate = None;
    loop {
        if !cfunc && peek_ident(input, "cfunc") {
            input.parse::<Ident>()?;
            cfunc = true;
        } else if !allocs && peek_ident(input, "allocs") {
            input.parse::<Ident>()?;
            allocs = true;
        } else if !inherits && peek_ident(input, "inherits") {
            input.parse::<Ident>()?;
            inherits = true;
        } else if !hidden && peek_ident(input, "hidden") {
            input.parse::<Ident>()?;
            hidden = true;
        } else if gate.is_none() && peek_ident(input, "gated") {
            input.parse::<Ident>()?; // `gated`
            let env = peek_ident(input, "env");
            if env {
                input.parse::<Ident>()?;
            }
            let lit: syn::LitStr = input.parse()?;
            gate = Some(Gate {
                env,
                feature: lit.value(),
            });
        } else {
            break;
        }
    }

    let buf;
    parenthesized!(buf in input);
    let recv: Ident = buf.parse()?;
    let (params, rest, keywords, kwrest, kwrest_strict, block) = parse_params(&buf)?;

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
        keywords,
        ruby_sig,
        kwrest,
        kwrest_strict,
        block,
        cfunc,
        allocs,
        inherits,
        hidden,
        gate,
        body,
    })
}

/// Parse the parameters after the receiver slot:
/// `required* optional* [*rest] keyword* [**kwrest] [&block]`.
///
/// Order is enforced here rather than left to the reader, because the order is
/// what makes the argument-count guard and the reported arity derivable at all.
/// Post-required parameters (`(a, *r, b)`) are rejected: no builtin needs them,
/// and they would make the guard a two-sided split for no gain.
#[expect(
    clippy::type_complexity,
    reason = "one tuple per parsed parameter list, read at one call site"
)]
fn parse_params(
    input: ParseStream,
) -> syn::Result<(
    Vec<Param>,
    Option<Ident>,
    Vec<KwParam>,
    Option<Ident>,
    bool,
    Option<Ident>,
)> {
    let mut params: Vec<Param> = Vec::new();
    let mut rest = None;
    let mut keywords: Vec<KwParam> = Vec::new();
    let mut kwrest = None;
    let mut kwrest_strict = false;
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
            // A trailing `!` asks for the strict peel -- see
            // `MethodDef::kwrest_strict`.
            if input.peek(Token![!]) {
                input.parse::<Token![!]>()?;
                kwrest_strict = true;
            }
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

        // `k:` / `k: EXPR` / `k:?` -- a NAMED keyword. Told from a positional
        // by the `:` that follows the name, so it is peeked before the
        // positional arm claims the ident.
        if input.peek(Ident) && input.peek2(Token![:]) && !input.peek2(Token![::]) {
            let name: Ident = input.parse()?;
            input.parse::<Token![:]>()?;
            let kind = if input.peek(Token![?]) {
                input.parse::<Token![?]>()?;
                KwKind::Maybe
            } else if input.peek(Token![,]) || input.is_empty() {
                KwKind::Required
            } else {
                KwKind::Optional(Box::new(input.parse()?))
            };
            if keywords.iter().any(|k| k.name == name) {
                return Err(syn::Error::new(name.span(), "duplicate keyword parameter"));
            }
            if block.is_some() || kwrest.is_some() {
                return Err(syn::Error::new(
                    name.span(),
                    "keyword parameters must come before `**kwrest` and `&block`",
                ));
            }
            keywords.push(KwParam { name, kind });
            continue;
        }

        if block.is_some() || kwrest.is_some() || rest.is_some() || !keywords.is_empty() {
            return Err(input.error(
                "positional parameters must come before `*rest`, keywords, `**kwrest` and `&block`",
            ));
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

    Ok((params, rest, keywords, kwrest, kwrest_strict, block))
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

    /// Every shape a `params "..."` spelling has to carry, checked against the
    /// oracle's own answer for a row that has it.
    #[test]
    fn a_signature_spelling_is_rubys_own_parameters_answer() {
        let k = |s: &str| {
            parse_signature(s)
                .unwrap()
                .into_iter()
                .map(|p| (p.kind, p.name))
                .collect::<Vec<_>>()
        };
        let n = |s: &str| Some(s.to_string());
        // `Array#pack` -> [[:req, :fmt], [:key, :buffer]]
        assert_eq!(
            k("fmt, buffer: nil"),
            vec![(SigKind::Req, n("fmt")), (SigKind::Key, n("buffer"))]
        );
        // `GC.start` -> three optional keywords, no positional.
        assert_eq!(
            k("full_mark: true, immediate_mark: true, immediate_sweep: true"),
            vec![
                (SigKind::Key, n("full_mark")),
                (SigKind::Key, n("immediate_mark")),
                (SigKind::Key, n("immediate_sweep")),
            ]
        );
        // A required keyword is the one with NO default.
        assert_eq!(k("into:"), vec![(SigKind::KeyReq, n("into"))]);
        // The anonymous forwarding trio is named for its own sigil, which is
        // what `Ractor#send` reports.
        assert_eq!(
            k("*, **, &"),
            vec![
                (SigKind::Rest, n("*")),
                (SigKind::KeyRest, n("**")),
                (SigKind::Block, n("&")),
            ]
        );
        assert_eq!(
            k("*args, **opts, &blk"),
            vec![
                (SigKind::Rest, n("args")),
                (SigKind::KeyRest, n("opts")),
                (SigKind::Block, n("blk")),
            ]
        );
        // `Dir.glob`: an optional positional whose default carries a `::`, and
        // one carrying a top-level comma inside brackets -- neither may split
        // the list or read as a keyword.
        assert_eq!(
            k("pattern, _flags = 0, base: nil"),
            vec![
                (SigKind::Req, n("pattern")),
                (SigKind::Opt, n("_flags")),
                (SigKind::Key, n("base")),
            ]
        );
        assert_eq!(
            k("enc = Encoding::UTF_8, seed = [1, 2]"),
            vec![(SigKind::Opt, n("enc")), (SigKind::Opt, n("seed"))]
        );
        // A parameter genuinely named `_` (`Process::Tms#stime=`).
        assert_eq!(k("_"), vec![(SigKind::Req, n("_"))]);
    }

    /// The arity a spelling implies is the one CRuby derives from the same
    /// signature -- which is why a row that spells its parameters needs no
    /// `arity N` override.
    #[test]
    fn arity_falls_out_of_the_signature() {
        let a = |s: &str| signature_arity(&parse_signature(s).unwrap());
        assert_eq!(a(""), 0);
        assert_eq!(a("a, b"), 2);
        assert_eq!(a("a, b = nil"), -2);
        assert_eq!(a("*args"), -1);
        // `GC.start`: optional keywords alone make it variadic.
        assert_eq!(a("full_mark: true"), -1);
        // A REQUIRED keyword adds one mandatory slot and keeps it fixed.
        assert_eq!(a("a, b:"), 2);
        // A block never counts.
        assert_eq!(a("a, &blk"), 1);
    }

    #[test]
    fn a_signature_that_is_not_a_signature_is_an_error() {
        assert!(parse_signature("a b").is_err());
        assert!(parse_signature("a,,b").is_err());
    }

    /// The keyword grammar, and the descriptor a `ruby def` derives from it.
    #[test]
    fn a_ruby_def_derives_its_own_parameters() {
        let spec = parse_class(quote! {
            A = A_CLASS;
            ruby def "sample"(recv, n?, random:?) { }
            ruby def "glob"(recv, pattern, flags = 0, *extra, base:?, sort: true, **rest, &blk) { }
            ruby def "need"(recv, k:) { }
            def "plain"(recv, arg, **opts) { }
        });
        let d = |i: usize| {
            spec.methods[i]
                .derived_params()
                .map(|v| v.into_iter().map(|p| (p.kind, p.name)).collect::<Vec<_>>())
        };
        let n = |s: &str| Some(s.to_string());
        assert_eq!(
            d(0),
            Some(vec![(SigKind::Opt, n("n")), (SigKind::Key, n("random"))])
        );
        // Ruby's canonical order, whatever order the reader expects:
        // positionals, rest, keywords, keyrest, block.
        assert_eq!(
            d(1),
            Some(vec![
                (SigKind::Req, n("pattern")),
                (SigKind::Opt, n("flags")),
                (SigKind::Rest, n("extra")),
                (SigKind::Key, n("base")),
                (SigKind::Key, n("sort")),
                (SigKind::KeyRest, n("rest")),
                (SigKind::Block, n("blk")),
            ])
        );
        assert_eq!(d(2), Some(vec![(SigKind::KeyReq, n("k"))]));
        // An UNMARKED def derives nothing -- its list describes how the body
        // receives arguments, not what ruby reports.
        assert_eq!(d(3), None);
    }

    /// A `ruby def`'s arity comes from the signature it declares, so the two
    /// reflection answers cannot disagree.
    #[test]
    fn a_ruby_defs_arity_comes_from_its_signature() {
        let spec = parse_class(quote! {
            A = A_CLASS;
            ruby def "sample"(recv, n?, random:?) { }
            ruby def "need"(recv, a, k:) { }
            ruby def "two"(recv, a, b) { }
            ruby def "blk"(recv, a, &b) { }
        });
        let a = |i: usize| spec.methods[i].derived_arity();
        assert_eq!(a(0), -1, "an optional positional makes it variadic");
        assert_eq!(a(1), 2, "a REQUIRED keyword adds one mandatory slot");
        assert_eq!(a(2), 2);
        assert_eq!(a(3), 1, "a block never counts");
    }

    #[test]
    fn keywords_come_after_the_positionals_and_before_the_rest() {
        let bad = ClassSpec::parse_class.parse2(quote! {
            A = A_CLASS;
            ruby def "x"(recv, k:, tail) { }
        });
        assert!(bad.is_err());
        let dup = ClassSpec::parse_class.parse2(quote! {
            A = A_CLASS;
            ruby def "x"(recv, k:, k:) { }
        });
        assert!(dup.is_err());
    }

    #[test]
    fn default_arity_is_none_meaning_variadic() {
        let spec = parse_module(quote! {
            M = MATH_CLASS;
            def "sqrt"(_recv, _args, _block) { Ok(RubyValue::Nil) }
        });
        assert_eq!(spec.methods[0].names[0].arity, None);
    }

    /// Two bare parameters mean two required arguments, never a
    /// `(recv, args, block)` triple; the variadic header is spelled
    /// explicitly as `(recv, *args, &block)`.
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
