//! `Symbol` (CRuby symbol.c/string.c) -- the Tier A surface. The
//! case/succ rows delegate to `string.rs`'s shared helpers and re-intern;
//! `to_proc` builds the `&:name` block (one dynamic dispatch per call).

use crate::builtins::arg_error;
use crate::builtins::inherited_row;
use crate::{RProc, RubyValue, Symbol};
use zeo_macros::ruby_class;

fn recv_sym(recv: &RubyValue) -> Symbol {
    match recv {
        RubyValue::Symbol(s) => *s,
        _ => unreachable!("Symbol table row dispatched on a non-Symbol receiver"),
    }
}

/// Every operator method name that prints as a bare symbol (`:+`, `:<=>`,
/// `:[]=`, `` :` ``) rather than a quoted one.
const OPERATOR_NAMES: &[&str] = &[
    "+", "-", "*", "/", "%", "**", "==", "===", "!=", "=~", "!~", "<", "<=", ">", ">=", "<=>",
    "<<", ">>", "&", "|", "^", "~", "!", "+@", "-@", "[]", "[]=", "`",
];

/// Whether a symbol name must be quoted in `inspect` (`:"a b"`) rather than
/// printed bare (`:abc`). Mirrors CRuby's `rb_str_symname_p`: a name prints
/// bare when it is an operator method, a plain identifier/constant, an
/// `@ivar`/`@@cvar`/`$gvar`, or a method name with a single trailing `?`, `!`,
/// or `=`. Everything else (spaces, leading digits, empty, punctuation) quotes.
/// Non-ASCII letters count as identifier characters, so `:café` and `:λ` print
/// bare while `:"😀"` (a non-letter) quotes.
pub(crate) fn needs_quoting(name: &str) -> bool {
    if name.is_empty() {
        return true;
    }
    if OPERATOR_NAMES.contains(&name) {
        return false;
    }
    // Sigil-prefixed names: an @ivar, @@cvar, or $gvar whose remainder is a
    // plain identifier prints bare; the bare sigil (`:@`) does not.
    for sigil in ["@@", "@", "$"] {
        if let Some(rest) = name.strip_prefix(sigil) {
            return !is_plain_ident(rest);
        }
    }
    if is_plain_ident(name) {
        return false;
    }
    // A method name may carry one trailing `?`, `!`, or `=` (`foo?`, `baz=`);
    // the character must be last (`foo?bar` still quotes).
    if let Some(body) = name
        .strip_suffix(['?', '!', '='])
        .filter(|b| !b.is_empty() && is_plain_ident(b))
    {
        let _ = body;
        return false;
    }
    true
}

/// A bare Ruby identifier: an underscore, an ASCII letter, or ANY non-ASCII
/// character to start, then underscores/ASCII-alphanumerics/non-ASCII. Ruby
/// treats every multibyte character as an identifier character in a symbol
/// name, so `:café`, `:λ`, and even `:😀` print bare, while ASCII punctuation
/// (a space, a leading digit) forces quoting.
fn is_plain_ident(s: &str) -> bool {
    let is_start = |c: char| c == '_' || c.is_ascii_alphabetic() || !c.is_ascii();
    let is_cont = |c: char| c == '_' || c.is_ascii_alphanumeric() || !c.is_ascii();
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if is_start(c) => {}
        _ => return false,
    }
    chars.all(is_cont)
}

/// A symbol's `inspect` form: `:name` when the name prints bare, else `:"..."`
/// with the name escaped exactly as `String#inspect` would.
pub(crate) fn inspect_name(name: &str) -> String {
    if needs_quoting(name) {
        format!(":{}", quoted_name(name))
    } else {
        format!(":{name}")
    }
}

/// A symbol name escaped and quoted like a string literal (`a b` -> `"a b"`),
/// reusing the encoding-aware string inspector so control characters, quotes,
/// and non-ASCII text match `String#inspect` byte-for-byte.
pub(crate) fn quoted_name(name: &str) -> String {
    crate::encoding::inspect(&crate::encoding::StrBuf::from_utf8(name.to_string()))
}

/// A symbol hash key in the `name:` shorthand: bare when the name prints bare
/// (`{a: 1}`), otherwise quoted (`{"k space": 2}`).
pub(crate) fn hash_key(name: &str) -> String {
    if needs_quoting(name) {
        quoted_name(name)
    } else {
        name.to_string()
    }
}

/// A Struct/Data member label in `#<struct ...>` inspect: bare when the name is
/// a plain local-variable identifier (`name=`), otherwise the member's symbol
/// literal (`:verbose?=`, `:+=`). This is stricter than `inspect_name`'s bare
/// rule -- an operator or `?`/`!`-suffixed member still takes the leading colon.
pub(crate) fn struct_member_label(name: &str) -> String {
    if is_plain_ident(name) {
        name.to_string()
    } else {
        inspect_name(name)
    }
}

/// Delegates a name-reading Symbol method to the same-named `String` method,
/// evaluated over the symbol's name (`:foo.start_with?("f")` ==
/// `"foo".start_with?("f")`).
fn sym_via_name(
    recv: &RubyValue,
    method: &str,
    args: &[RubyValue],
    block: Option<RubyValue>,
) -> Result<RubyValue, crate::Signal> {
    let name = RubyValue::Str(crate::string_new(recv_sym(recv).name()));
    crate::dispatch::send_value(&name, Symbol::intern(method), args, block)
}

/// `Symbol#to_proc`'s conversion -- also the `&:name` block-argument path
/// (`emit_block_option` routes every `&expr` through
/// `block_arg_to_proc`).
pub(crate) fn symbol_to_proc(name: Symbol) -> RubyValue {
    // CRuby reports `:name.to_proc.arity` as -2 (one required receiver plus
    // optional trailing args), and `:name.to_proc.lambda?` as true.
    // Built with the BLOCK-aware constructor: a symbol proc forwards whatever
    // block it was called with, so `:map.to_proc.call([1,2]) { |x| x * 3 }`
    // runs the block instead of answering an Enumerator. Dropping the block
    // was invisible until something called the proc with one.
    let p: RProc = RProc::with_self_and_block(
        move |_self: &RubyValue, args: &[RubyValue], block: Option<RubyValue>| {
            let Some((recv, rest)) = args.split_first() else {
                return Err(arg_error!("no receiver given"));
            };
            crate::dispatch::send_value(recv, name, rest, block)
        },
        RubyValue::Nil,
        -2,
        true,
    );
    RubyValue::Proc(p.with_symbol_origin(name))
}

ruby_class! {
    Symbol = zeo_abi::SYMBOL_CLASS < zeo_abi::OBJECT_CLASS;
    include zeo_abi::COMPARABLE_CLASS;

    // `Symbol.all_symbols` -- every symbol interned so far, in intern order.
    // Nothing is ever removed from the interner, so this only grows.
    def self."all_symbols"(_recv) {
        let all = (0..crate::Symbol::count())
            .map(|i| RubyValue::Symbol(crate::Symbol::from_u32(i as u32)))
            .collect();
        Ok(RubyValue::Array(crate::array_new(all)))
    }

    def "to_s" | "id2name" (recv) {
        // The EXACT interned bytes and encoding, not the lossy text -- a
        // symbol minted from a non-UTF-8 string spells it back verbatim.
        let sym = recv_sym(recv);
        Ok(RubyValue::Str(crate::string_from_bytes(
            sym.bytes().to_vec(),
            sym.encoding(),
        )))
    }
    // `Symbol#name` returns a FROZEN String (unlike `to_s`, which is a fresh
    // mutable copy) -- CRuby caches and freezes it.
    def "name" (recv) {
        let sym = recv_sym(recv);
        let s = crate::string_from_bytes(sym.bytes().to_vec(), sym.encoding());
        s.set_frozen();
        Ok(RubyValue::Str(s))
    }
    def "to_sym" | "intern" (recv) {
        Ok(recv.clone())
    }
    def "encoding" (recv) {
        Ok(crate::builtins::encoding::encoding_value(recv_sym(recv).encoding()))
    }
    def "inspect" (recv) {
        Ok(RubyValue::Str(crate::string_new(inspect_name(&recv_sym(recv).name()))))
    }
    def "length" | "size" (recv) {
        Ok(RubyValue::Int(recv_sym(recv).name().chars().count() as i64))
    }
    def "empty?" (recv) {
        Ok(RubyValue::Bool(recv_sym(recv).name().is_empty()))
    }
    def "<=>" (recv, other) {
        let RubyValue::Symbol(other) = other else {
            return Ok(RubyValue::Nil);
        };
        Ok(RubyValue::Int(
            recv_sym(recv).name().cmp(&other.name()) as i64
        ))
    }
    def "==" (recv, other) {
        Ok(RubyValue::Bool(recv.rb_eq(other)))
    }
    // `sym[...]` reads a substring of the symbol's NAME, returning a String
    // (or nil) -- identical to `sym.to_s[...]`, so it delegates to `String#[]`
    // for the full index/length/range/regexp surface.
    def "[]" | "slice"(recv, *args, &_block) {
        let name = RubyValue::Str(crate::string_new(recv_sym(recv).name()));
        crate::dispatch::send_value(&name, crate::Symbol::intern("[]"), args, None)
    }
    // These read the symbol's NAME as a string, so they delegate to the
    // matching `String` method (a Symbol is name-plus-identity).
    def "=~" arity 1 (recv, *args, &block) { sym_via_name(recv, "=~", args, block) }
    def "match"(recv, *args, &block) { sym_via_name(recv, "match", args, block) }
    def "match?"(recv, *args, &block) { sym_via_name(recv, "match?", args, block) }
    def "start_with?"(recv, *args, &block) { sym_via_name(recv, "start_with?", args, block) }
    def "end_with?"(recv, *args, &block) { sym_via_name(recv, "end_with?", args, block) }
    // Case-insensitive name comparison. `casecmp` answers -1/0/1 (nil if the
    // argument isn't a Symbol); `casecmp?` answers true/false/nil.
    // The String rule, and for the same reason -- see `String#casecmp`:
    // ASCII-only folding over BYTES, with `casecmp?` carrying the Unicode
    // comparison.
    def "casecmp" (recv, arg) {
        let RubyValue::Symbol(other) = arg else {
            return Ok(RubyValue::Nil);
        };
        let (a, b) = (recv_sym(recv).name(), other.name());
        Ok(RubyValue::Int(
            crate::builtins::string::ascii_casecmp(a.as_bytes(), b.as_bytes()) as i64,
        ))
    }
    def "casecmp?" (recv, arg) {
        let RubyValue::Symbol(other) = arg else {
            return Ok(RubyValue::Nil);
        };
        Ok(RubyValue::Bool(
            recv_sym(recv).name().to_lowercase() == other.name().to_lowercase(),
        ))
    }
    def "upcase" cfunc (recv) {
        Ok(RubyValue::Symbol(Symbol::intern(
            &recv_sym(recv).name().to_uppercase(),
        )))
    }
    def "downcase" cfunc (recv) {
        Ok(RubyValue::Symbol(Symbol::intern(
            &recv_sym(recv).name().to_lowercase(),
        )))
    }
    def "capitalize" cfunc (recv) {
        Ok(RubyValue::Symbol(Symbol::intern(
            &crate::builtins::string::capitalize_str(&recv_sym(recv).name()),
        )))
    }
    def "swapcase" cfunc (recv) {
        Ok(RubyValue::Symbol(Symbol::intern(
            &crate::builtins::string::swapcase_str(&recv_sym(recv).name()),
        )))
    }
    def "succ" | "next" (recv) {
        Ok(RubyValue::Symbol(Symbol::intern(
            &crate::builtins::string::succ_str(&recv_sym(recv).name()),
        )))
    }
    def "to_proc" (recv) {
        Ok(symbol_to_proc(recv_sym(recv)))
    }

    // ---- rows ruby OWNS on this class while the body lives on an ancestor.
    // Each calls the very row it would otherwise have inherited, so `.owner`
    // and `instance_methods(false)` agree and there is still only one body.
    def "==="(recv, _other) { inherited_row!(kernel, "===", recv, __args, None) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(s: &str) -> RubyValue {
        RubyValue::Symbol(Symbol::intern(s))
    }

    /// The `ruby_class!`-generated instance methods are reachable only through
    /// the dispatch table (their Rust fn names are mangled), so the tests call
    /// them the way real dispatch does -- through Symbol's registered lookup.
    fn imethod(name: &str) -> crate::builtins::BuiltinMethodFn {
        let tbl = crate::builtins::registered_table(zeo_abi::SYMBOL_CLASS)
            .expect("Symbol is a registered builtin table")
            .instance
            .as_ref()
            .expect("Symbol has instance methods");
        (tbl.lookup)(name).unwrap_or_else(|| panic!("Symbol#{name} is defined"))
    }

    #[test]
    fn reflection_rows_match_the_oracle() {
        let r = imethod("inspect")(&sym("he"), &[], None).unwrap();
        assert_eq!(r.to_display_string(), ":he");
        let r = imethod("length")(&sym("hello"), &[], None).unwrap();
        assert!(matches!(r, RubyValue::Int(5)));
        let r = imethod("<=>")(&sym("b"), &[sym("a")], None).unwrap();
        assert!(matches!(r, RubyValue::Int(1)));
        let r = imethod("upcase")(&sym("he"), &[], None).unwrap();
        assert_eq!(r.inspect_string(), ":HE");
        let r = imethod("succ")(&sym("a"), &[], None).unwrap();
        assert_eq!(r.inspect_string(), ":b");
    }

    #[test]
    fn inspect_quotes_only_non_bare_names() {
        // Bare: identifiers, constants, sigils, suffixed methods, operators,
        // and any non-ASCII characters (letters or symbols like an emoji).
        for bare in [
            "abc", "Foo", "_x9", "@iv", "@@cv", "$g", "foo?", "baz=", "+", "<=>", "[]=", "`",
            "café", "λ", "😀",
        ] {
            assert!(!needs_quoting(bare), "{bare:?} should print bare");
            assert_eq!(inspect_name(bare), format!(":{bare}"));
        }
        // Quoted: spaces, leading digit, empty, embedded suffix char, bare sigil.
        for q in ["a b", "1x", "", "foo?bar", "@"] {
            assert!(needs_quoting(q), "{q:?} should quote");
        }
        assert_eq!(inspect_name("a b"), ":\"a b\"");
        assert_eq!(inspect_name(""), ":\"\"");
        assert_eq!(hash_key("normal"), "normal");
        assert_eq!(hash_key("k space"), "\"k space\"");
    }

    #[test]
    fn to_proc_dispatches_the_named_method() {
        let p = symbol_to_proc(Symbol::intern("length"));
        let RubyValue::Proc(p) = p else { panic!() };
        let s = RubyValue::Str(crate::string_new("abc".to_string()));
        let r = p.call(&[s]).unwrap();
        assert!(matches!(r, RubyValue::Int(3)));
    }
}
