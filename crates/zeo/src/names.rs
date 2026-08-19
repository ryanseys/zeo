//! The backend-neutral name-mangling core: how a Ruby name (method/class/
//! local/ivar) becomes a valid target-language identifier FRAGMENT. The
//! Rust-specific half (keyword escaping, `r#` raw idents, `proc_macro2::Ident`
//! construction) stays in `codegen::ident`; a CLIF symbol namer composes the
//! same fragments.

/// A Ruby method name that's a bare operator symbol (`def +`/`def <=>`/
/// `a + b`/`a <=> b` are the exact same method name either way -- operators
/// are ordinary `Call`s) -- none of
/// these are valid Rust identifier TEXT at all (`+`, `<=>`, `[]`, ...), so
/// unlike `escape_special_suffix`'s plain-name-plus-suffix rewriting, this
/// is a fixed, exhaustive lookup table, not a general escaping rule.
/// Without it, a user class defining `def +(other)`/`def <=>(other)`
/// panics at codegen time with a raw `proc_macro2` "not a valid Ident"
/// error. Both the `impl` block's method
/// definition (`codegen::mod::emit_class`) and a call site's general
/// (non-fast-path) dispatch (`codegen::call::dispatch`) route every method
/// name through this SAME table, so fixing it once makes both sides
/// agree automatically.
pub(crate) const OPERATOR_METHOD_NAMES: &[(&str, &str)] = &[
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

/// A spelling of `name` that Rust accepts as an identifier, or `None` when it
/// already is one.
///
/// Ruby's identifier rule is far wider than Rust's: every byte above ASCII
/// counts, so `@height´` is a legal ivar (fastimage ships one), as is any
/// Latin-1 punctuation in a method name. Rust wants XID characters, and
/// `Ident::new` PANICS on anything else -- which reached the user as an
/// internal-error backtrace instead of a diagnostic.
///
/// Each rejected character becomes its `_uXXXX_` code point, so a name stays
/// recognizable and two names cannot collide: the encoding is reversible.
pub(crate) fn transliterate(name: &str) -> Option<String> {
    let ok = |c: char, first: bool| {
        c == '_'
            || if first {
                unicode_ident::is_xid_start(c)
            } else {
                unicode_ident::is_xid_continue(c)
            }
    };
    if name.chars().enumerate().all(|(i, c)| ok(c, i == 0)) {
        return None;
    }
    let mut out = String::with_capacity(name.len());
    for (i, c) in name.chars().enumerate() {
        if ok(c, i == 0 && out.is_empty()) {
            out.push(c);
        } else {
            out.push_str(&format!("_u{:04X}_", c as u32));
        }
    }
    Some(out)
}

/// A class/module name as an ident FRAGMENT -- the part a mangling arm
/// interpolates after its own prefix.
///
/// Every class name zeo SYNTHESIZES is deliberately unspellable as a Ruby
/// constant, so the holder claims no constant of its own: a refinement holder
/// is `#refinement:String` (the very name CRuby prints for
/// `M.refinements.first`), an anonymous `using` module is `#using:<offset>`,
/// and a `class << self` surrogate is `#<Class:self>`. None of `#`, `:`, `<`,
/// `>` is a Rust XID character, so interpolating one raw makes `Ident::new`
/// PANIC -- which reached users as an internal-error backtrace instead of a
/// diagnostic (`"__c405_#refinement:String" is not a valid Ident`).
///
/// The names must not change: they are Ruby-visible through the reflection
/// methods. So the IDENT is sanitized here instead, at the one place a name
/// becomes one.
///
/// `transliterate` answers `None` for a name that already is an identifier, so
/// every ordinary class keeps its exact spelling and generated code for a
/// program with no synthesized name stays byte-identical.
///
/// The mapping is INJECTIVE, which is why it does not fold `::` to `_` the way
/// the callers used to. `Hello::World` and `Hello_World` are both ordinary
/// Ruby names, and `.replace("::", "_")` sends them to the SAME fragment --
/// two different classes with one generated ident. Today every caller happens
/// to prefix a unique `ClassId` or class index, so the collision could not
/// surface; relying on that is a trap for the next caller. Escaping `::` as
/// `_u003A__u003A_` costs readability only for the rare qualified builtin name
/// (`Enumerator::Yielder`) and cannot collide with anything.
pub(crate) fn ident_fragment(name: &str) -> String {
    match transliterate(name) {
        Some(safe) => safe,
        None => name.to_string(),
    }
}

/// `foo?` -> `foo_p`, `foo!` -> `foo_bang`, `foo=` -> `foo_set` -- but NOT
/// operator method names that happen to end the same way (`==`, `!=`, `<=`,
/// `>=`, `[]=`), which are fixed multi-char symbol sequences with no
/// identifier-like base, not a plain name plus a suffix -- those are handled
/// by [`OPERATOR_METHOD_NAMES`], checked before this function is ever
/// called.
///
/// A marker is a RESERVED ending, so a plain Ruby name that already ends in
/// one gets a trailing `_`: `remote=` and `remote_set` are two different
/// methods, and rubygems' `Gem::Resolver::InstallerSet` defines both (a
/// "duplicate definitions with name `remote_set`" on the generated program --
/// a miscompile of legal input, exactly like `class_method_ident`'s). Since a
/// marker never ends in `_`, the escaped form can't collide back.
pub(crate) fn escape_special_suffix(name: &str) -> Option<String> {
    const MARKERS: [(&str, &str); 3] = [("?", "_p"), ("!", "_bang"), ("=", "_set")];
    fn is_ident_like(base: &str) -> bool {
        let mut chars = base.chars();
        matches!(chars.next(), Some(c) if c.is_alphabetic() || c == '_')
            && chars.all(|c| c.is_alphanumeric() || c == '_')
    }
    for (suffix, marker) in MARKERS {
        if let Some(base) = name.strip_suffix(suffix)
            && is_ident_like(base)
        {
            return Some(format!("{base}{marker}"));
        }
    }
    if is_ident_like(name) && MARKERS.iter().any(|(_, m)| name.ends_with(m)) {
        return Some(format!("{name}_"));
    }
    None
}
