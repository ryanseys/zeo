//! Turns a Ruby identifier (method/local/ivar/class name) into a
//! `proc_macro2::Ident`. Needed because a bare Rust reserved keyword (`type`,
//! `loop`, `match`, `yield`, ...) makes `Ident::new` panic -- unlike the old
//! string-based codegen, which would have silently emitted invalid Rust text
//! and only failed later, confusingly, inside `cargo build`. Raw-identifier
//! escaping (`r#type`) handles every keyword collision except the handful of
//! *weak*/path keywords (`self`, `Self`, `super`, `crate`) that Rust doesn't
//! allow as raw identifiers at all -- those aren't legal Ruby identifiers
//! either (`self` is itself a Ruby keyword), so they're not expected to reach
//! here; `safe_ident` panics with a clear message if one ever does, rather
//! than emitting `r#self` and letting `rustc` reject it confusingly later.
//!
//! A Ruby method name may also carry a trailing `?`/`!`/`=` (a predicate,
//! "dangerous"/mutating, or setter method -- `empty?`, `save!`, `value=`) --
//! none valid trailing characters in a Rust identifier. `escape_special_suffix`
//! re-suffixes those with a plain-ASCII marker instead.

use proc_macro2::{Ident, Span};

const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "static", "struct", "trait", "true", "type", "unsafe", "use", "where",
    "while", "abstract", "become", "box", "do", "final", "macro", "override", "priv", "typeof",
    "unsized", "virtual", "yield", "try",
];

const UNESCAPABLE: &[&str] = &["self", "Self", "super", "crate"];

/// A Ruby method name that's a bare operator symbol (`def +`/`def <=>`/
/// `a + b`/`a <=> b` are the exact same method name either way -- see the
/// plan's Phase 1 insight that operators are ordinary `Call`s) -- none of
/// these are valid Rust identifier TEXT at all (`+`, `<=>`, `[]`, ...), so
/// unlike `escape_special_suffix`'s plain-name-plus-suffix rewriting, this
/// is a fixed, exhaustive lookup table, not a general escaping rule. Found
/// as a real, previously-undetected gap (this session's own testing): a
/// user class defining `def +(other)`/`def <=>(other)` panicked at codegen
/// time with a raw `proc_macro2` "not a valid Ident" error, meaning
/// user-defined operator overloading -- long claimed to "work for free"
/// once operators became ordinary Calls (Phase 1) -- never actually worked
/// for the DEFINING side, only ever exercised via native `Int`/`Float` fast
/// paths that bypass `safe_ident` entirely. Both the `impl` block's method
/// definition (`codegen::mod::emit_class`) and a call site's general
/// (non-fast-path) dispatch (`codegen::call::dispatch`) route every method
/// name through this SAME function, so fixing it once makes both sides
/// agree automatically.
const OPERATOR_METHOD_NAMES: &[(&str, &str)] = &[
    ("+", "op_add"),
    ("-", "op_sub"),
    ("*", "op_mul"),
    ("/", "op_div"),
    ("%", "op_mod"),
    ("**", "op_pow"),
    ("==", "op_eq"),
    ("!=", "op_neq"),
    ("<", "op_lt"),
    (">", "op_gt"),
    ("<=", "op_le"),
    (">=", "op_ge"),
    ("<=>", "op_cmp"),
    ("&", "op_band"),
    ("|", "op_bor"),
    ("^", "op_bxor"),
    ("<<", "op_shl"),
    (">>", "op_shr"),
    ("[]", "op_index"),
    ("[]=", "op_index_set"),
    ("-@", "op_neg"),
    ("+@", "op_pos"),
    ("~", "op_bnot"),
    ("!", "op_not"),
    ("=~", "op_match"),
    ("!~", "op_not_match"),
    ("===", "op_case_eq"),
];

pub fn safe_ident(name: &str) -> Ident {
    if UNESCAPABLE.contains(&name) {
        panic!("`{name}` is not a valid Ruby identifier and can't be escaped as a Rust one");
    }
    if let Some(&(_, escaped)) = OPERATOR_METHOD_NAMES.iter().find(|&&(op, _)| op == name) {
        return Ident::new(escaped, Span::call_site());
    }
    if let Some(escaped) = escape_special_suffix(name) {
        // Recurse once, now suffix-free, so the escaped base still gets the
        // ordinary Rust-keyword check above (e.g. a hypothetical `type?`
        // would need `r#type_p`... except `_p` already makes it non-keyword;
        // recursing is what makes that reasoning automatic rather than
        // assumed).
        return safe_ident(&escaped);
    }
    if RUST_KEYWORDS.contains(&name) {
        Ident::new_raw(name, Span::call_site())
    } else {
        Ident::new(name, Span::call_site())
    }
}

/// `foo?` -> `foo_p`, `foo!` -> `foo_bang`, `foo=` -> `foo_set` -- but NOT
/// operator method names that happen to end the same way (`==`, `!=`, `<=`,
/// `>=`, `[]=`), which are fixed multi-char symbol sequences with no
/// identifier-like base, not a plain name plus a suffix -- those are handled
/// by `OPERATOR_METHOD_NAMES` above, checked before this function is ever
/// called.
fn escape_special_suffix(name: &str) -> Option<String> {
    fn is_ident_like(base: &str) -> bool {
        let mut chars = base.chars();
        matches!(chars.next(), Some(c) if c.is_alphabetic() || c == '_')
            && chars.all(|c| c.is_alphanumeric() || c == '_')
    }
    for (suffix, marker) in [("?", "_p"), ("!", "_bang"), ("=", "_set")] {
        if let Some(base) = name.strip_suffix(suffix) {
            if is_ident_like(base) {
                return Some(format!("{base}{marker}"));
            }
        }
    }
    None
}

/// The Rust identifier for a class/module's generated struct/`mod`/`impl`
/// -- the ONE place a resolved `ClassId` becomes a Rust name (Phase 15.1;
/// previously ~11 call sites each derived it from `ClassInfo.name`
/// independently). A top-level class keeps the plain `safe_ident(name)`
/// (generated code for flat programs stays byte-identical); a NESTED class
/// (Phase 15.3) mangles to `__c<id>_<leaf>` -- the `ClassId` makes it
/// collision-free by construction (two `Widget`s in different namespaces,
/// or a namespace path whose `_`-join would be ambiguous, can never
/// collide), the leaf keeps it readable, and the `__` prefix can't collide
/// with any top-level class (Ruby constants start with an uppercase
/// letter). Phase 18 adds the box dimension here the same way.
pub(super) fn class_ident(
    compiler: &crate::compiler::Compiler,
    cid: crate::compiler::ClassId,
) -> proc_macro2::Ident {
    let ci = compiler.class(cid);
    if ci.lexical_parent.is_some() {
        return proc_macro2::Ident::new(
            &format!("__c{}_{}", cid.0, ci.name),
            proc_macro2::Span::call_site(),
        );
    }
    // A reopened BUILTIN's container `pub mod` (Phase 16.3 -- the only
    // generated item a builtin ever gets, see `emit_builtin_reopen`) is
    // prefix-mangled: a bare `pub mod String` would shadow the prelude
    // `String` TYPE at the crate root (Rust modules and types share a
    // namespace), breaking every generated body that names `String`.
    // `Object` stays un-mangled -- it never gets generated items at all
    // (reopening it is rejected), and its ident may still be referenced by
    // pre-16.3 paths expecting the plain name.
    if ci.is_builtin && cid != crate::compiler::OBJECT_CLASS {
        // A per-box OVERLAY (Phase 18) gets its own container module --
        // `__bm_b2_String` -- so a root reopen and any number of box
        // overlays of the same builtin coexist.
        if ci.box_id != 0 {
            return proc_macro2::Ident::new(
                &format!("__bm_b{}_{}", ci.box_id, ci.name),
                proc_macro2::Span::call_site(),
            );
        }
        return proc_macro2::Ident::new(
            &format!("__bm_{}", ci.name),
            proc_macro2::Span::call_site(),
        );
    }
    // A class DEFINED IN a box (Phase 18): `__b<box>_<leaf>` -- the box id
    // disambiguates it from a same-named main-program class (the ClassId
    // isn't needed: one box defines each top-level name at most once, and
    // nested classes already took the `__c<id>_` arm above).
    if ci.box_id != 0 {
        return proc_macro2::Ident::new(
            &format!("__b{}_{}", ci.box_id, ci.name),
            proc_macro2::Span::call_site(),
        );
    }
    safe_ident(&ci.name)
}
