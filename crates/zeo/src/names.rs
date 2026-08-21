//! The name-mangling core: how a Ruby name (method/class/local/ivar) becomes
//! a valid identifier FRAGMENT. `clif::names` composes these fragments into
//! the symbols an object file carries.

/// A spelling of `name` an identifier grammar accepts, or `None` when it
/// already is one.
///
/// Ruby's identifier rule is far wider: every byte above ASCII counts, so
/// `@height´` is a legal ivar (fastimage ships one), as is any Latin-1
/// punctuation in a method name.
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
