//! `Symbol` (CRuby symbol.c/string.c) -- the Tier A surface. The
//! case/succ rows delegate to `string.rs`'s shared helpers and re-intern;
//! `to_proc` builds the `&:name` block (one dynamic dispatch per call).

use crate::builtins::{arity, builtin_methods};
use crate::{RProc, RubyValue, Symbol};

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
    let p: RProc = RProc::with_meta(
        move |args: &[RubyValue]| {
            let Some((recv, rest)) = args.split_first() else {
                return Err(crate::dispatch::raise_error(
                    "ArgumentError",
                    "no receiver given".to_string(),
                ));
            };
            crate::dispatch::send_value(recv, name, rest, None)
        },
        -2,
        true,
    );
    RubyValue::Proc(p)
}

builtin_methods! {
    pub(crate) fn lookup;

    "to_s"[0] | "id2name"[0] | "name"[0] => fn to_s(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::string_new(recv_sym(recv).name())))
    }
    "to_sym"[0] | "intern"[0] => fn to_sym(recv, args, _block) {
        arity!(args, 0);
        Ok(recv.clone())
    }
    "encoding"[0] => fn encoding_m(recv, args, _block) {
        arity!(args, 0);
        let id = crate::builtins::encoding::computed_encoding_of(&recv_sym(recv).name());
        Ok(crate::builtins::encoding::encoding_value(id))
    }
    "inspect"[0] => fn inspect(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Str(crate::string_new(inspect_name(&recv_sym(recv).name()))))
    }
    "length"[0] | "size"[0] => fn length(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Int(recv_sym(recv).name().chars().count() as i64))
    }
    "empty?"[0] => fn empty_p(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Bool(recv_sym(recv).name().is_empty()))
    }
    "<=>"[1] => fn spaceship(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Symbol(other) = &args[0] else {
            return Ok(RubyValue::Nil);
        };
        Ok(RubyValue::Int(
            recv_sym(recv).name().cmp(&other.name()) as i64
        ))
    }
    "=="[1] => fn eq(recv, args, _block) {
        arity!(args, 1);
        Ok(RubyValue::Bool(recv.rb_eq(&args[0])))
    }
    // `sym[...]` reads a substring of the symbol's NAME, returning a String
    // (or nil) -- identical to `sym.to_s[...]`, so it delegates to `String#[]`
    // for the full index/length/range/regexp surface.
    "[]" | "slice" => fn index(recv, args, _block) {
        let name = RubyValue::Str(crate::string_new(recv_sym(recv).name()));
        crate::dispatch::send_value(&name, crate::Symbol::intern("[]"), args, None)
    }
    // These read the symbol's NAME as a string, so they delegate to the
    // matching `String` method (a Symbol is name-plus-identity).
    "=~"[1] => fn match_op(recv, args, block) { sym_via_name(recv, "=~", args, block) }
    "match" => fn match_m(recv, args, block) { sym_via_name(recv, "match", args, block) }
    "match?" => fn match_p(recv, args, block) { sym_via_name(recv, "match?", args, block) }
    "start_with?" => fn start_with_p(recv, args, block) { sym_via_name(recv, "start_with?", args, block) }
    "end_with?" => fn end_with_p(recv, args, block) { sym_via_name(recv, "end_with?", args, block) }
    // Case-insensitive name comparison. `casecmp` answers -1/0/1 (nil if the
    // argument isn't a Symbol); `casecmp?` answers true/false/nil.
    "casecmp"[1] => fn casecmp(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Symbol(other) = &args[0] else {
            return Ok(RubyValue::Nil);
        };
        let ord = recv_sym(recv)
            .name()
            .to_lowercase()
            .cmp(&other.name().to_lowercase());
        Ok(RubyValue::Int(ord as i64))
    }
    "casecmp?"[1] => fn casecmp_p(recv, args, _block) {
        arity!(args, 1);
        let RubyValue::Symbol(other) = &args[0] else {
            return Ok(RubyValue::Nil);
        };
        Ok(RubyValue::Bool(
            recv_sym(recv).name().to_lowercase() == other.name().to_lowercase(),
        ))
    }
    "upcase" => fn upcase(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Symbol(Symbol::intern(
            &recv_sym(recv).name().to_uppercase(),
        )))
    }
    "downcase" => fn downcase(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Symbol(Symbol::intern(
            &recv_sym(recv).name().to_lowercase(),
        )))
    }
    "capitalize" => fn capitalize(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Symbol(Symbol::intern(
            &crate::builtins::string::capitalize_str(&recv_sym(recv).name()),
        )))
    }
    "swapcase" => fn swapcase(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Symbol(Symbol::intern(
            &crate::builtins::string::swapcase_str(&recv_sym(recv).name()),
        )))
    }
    "succ"[0] | "next"[0] => fn succ(recv, args, _block) {
        arity!(args, 0);
        Ok(RubyValue::Symbol(Symbol::intern(
            &crate::builtins::string::succ_str(&recv_sym(recv).name()),
        )))
    }
    "to_proc"[0] => fn to_proc(recv, args, _block) {
        arity!(args, 0);
        Ok(symbol_to_proc(recv_sym(recv)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(s: &str) -> RubyValue {
        RubyValue::Symbol(Symbol::intern(s))
    }

    #[test]
    fn reflection_rows_match_the_oracle() {
        let r = inspect(&sym("he"), &[], None).unwrap();
        assert_eq!(r.to_display_string(), ":he");
        let r = length(&sym("hello"), &[], None).unwrap();
        assert!(matches!(r, RubyValue::Int(5)));
        let r = spaceship(&sym("b"), &[sym("a")], None).unwrap();
        assert!(matches!(r, RubyValue::Int(1)));
        let r = upcase(&sym("he"), &[], None).unwrap();
        assert_eq!(r.inspect_string(), ":HE");
        let r = succ(&sym("a"), &[], None).unwrap();
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
