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
    "as", "async", "await", "break", "const", "continue", "dyn", "else", "enum", "extern", "false",
    "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref",
    "return", "static", "struct", "trait", "true", "type", "unsafe", "use", "where", "while",
    "abstract", "become", "box", "do", "final", "macro", "override", "priv", "typeof", "unsized",
    "virtual", "yield", "try",
];

const UNESCAPABLE: &[&str] = &["self", "Self", "super", "crate"];

/// Rust-prelude names a top-level user class must not shadow: generated code
/// and the `ruby_class!` expansion reference these UNQUALIFIED (`Vec<RubyValue>`,
/// `Option<...>`, `Ok(...)`, `Send + Sync` bounds, ...), so a Ruby
/// `class Vec ... end` defining a real struct `Vec` at the crate root breaks
/// every such reference file-wide. Ruby BUILTINS that share a name (String,
/// Hash, ...) never reach this check -- they take `class_ident`'s `__bm_`
/// reopen arm first.
const RUST_PRELUDE_COLLISIONS: &[&str] = &[
    "Option",
    "Some",
    "None",
    "Result",
    "Ok",
    "Err",
    "String",
    "Vec",
    "Box",
    "Clone",
    "Copy",
    "Debug",
    "Default",
    "Drop",
    "Eq",
    "PartialEq",
    "Ord",
    "PartialOrd",
    "Hash",
    "Iterator",
    "IntoIterator",
    "DoubleEndedIterator",
    "ExactSizeIterator",
    "Extend",
    "Fn",
    "FnMut",
    "FnOnce",
    "From",
    "Into",
    "TryFrom",
    "TryInto",
    "AsRef",
    "AsMut",
    "Send",
    "Sync",
    "Sized",
    "Unpin",
    "ToOwned",
    "ToString",
    "FromIterator",
    "Future",
    "IntoFuture",
];

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
        panic!(
            "internal error: `{name}` is a Rust path keyword with no raw-identifier form and isn't a legal Ruby identifier either, so safe_ident should never receive it"
        );
    }
    // Ruby's `_` is an ordinary (readable) local; Rust's `_` is not a named
    // binding at all (`let mut _` won't parse, macro `$x:ident` matchers
    // reject it). Same `__`-reserved-name residual-risk posture as
    // `__blk`/`__self`.
    if name == "_" {
        return Ident::new("__underscore", Span::call_site());
    }
    if let Some(&(_, escaped)) = OPERATOR_METHOD_NAMES.iter().find(|&&(op, _)| op == name) {
        return Ident::new(escaped, Span::call_site());
    }
    // The escape runs exactly ONCE -- its own output ends in a marker, so
    // feeding it back would escape it a second time and undo the very
    // separation it exists to create (`remote=` and `remote_set` would meet
    // again at `remote_set_`). The keyword check still applies to whatever
    // comes out (a hypothetical `type?` would need `r#type_p`... except `_p`
    // already makes it non-keyword).
    keyword_safe(&escape_special_suffix(name).unwrap_or_else(|| name.to_string()))
}

/// A Rust keyword can only be spelled as an identifier in its raw form.
fn keyword_safe(name: &str) -> Ident {
    if RUST_KEYWORDS.contains(&name) {
        Ident::new_raw(name, Span::call_site())
    } else {
        Ident::new(name, Span::call_site())
    }
}

/// The Rust identifier for a CLASS method (`def self.x`), as distinct from
/// an instance method of the same Ruby name.
///
/// Ruby keeps instance and class methods in two separate namespaces, so
/// `def x` and `def self.x` on one class is ordinary, legal code:
///
/// ```ruby
/// class C
///   def self.x = "class-level"
///   def x      = "instance-level"
/// end
/// ```
///
/// The generated code has no such separation -- both land in the same `impl
/// C` (or, for a module/builtin reopen, the same `pub mod`) -- so emitting
/// both under the bare name is a `rustc` "duplicate definitions with name
/// `x`" error on the GENERATED program, i.e. a miscompile of a legal input.
/// Prefixing the class-method half sidesteps it without touching the
/// Ruby-visible name (nothing outside generated code sees this ident, unlike
/// `class_ident`'s containers, which call sites must spell exactly).
///
/// `__`-prefixed, the same reserved-name convention as `__blk`/`__self`.
///
/// Built by prefixing `safe_ident`'s OUTPUT rather than its input, so every
/// escape that function already does is inherited rather than re-derived:
/// `def self.+` arrives here as `op_add` (prefixing the raw `+` would panic
/// in `Ident::new`), and `def self.type` as the raw ident `r#type` -- whose
/// `r#` is dropped, since `__cm_type` is not a keyword and `__cm_r#type`
/// would not parse.
pub fn class_method_ident(name: &str) -> Ident {
    let base = safe_ident(name).to_string();
    let base = base.strip_prefix("r#").unwrap_or(&base);
    Ident::new(&format!("__cm_{base}"), Span::call_site())
}

/// `foo?` -> `foo_p`, `foo!` -> `foo_bang`, `foo=` -> `foo_set` -- but NOT
/// operator method names that happen to end the same way (`==`, `!=`, `<=`,
/// `>=`, `[]=`), which are fixed multi-char symbol sequences with no
/// identifier-like base, not a plain name plus a suffix -- those are handled
/// by `OPERATOR_METHOD_NAMES` above, checked before this function is ever
/// called.
///
/// A marker is a RESERVED ending, so a plain Ruby name that already ends in
/// one gets a trailing `_`: `remote=` and `remote_set` are two different
/// methods, and rubygems' `Gem::Resolver::InstallerSet` defines both (a
/// "duplicate definitions with name `remote_set`" on the generated program --
/// a miscompile of legal input, exactly like `class_method_ident`'s). Since a
/// marker never ends in `_`, the escaped form can't collide back.
fn escape_special_suffix(name: &str) -> Option<String> {
    const MARKERS: [(&str, &str); 3] = [("?", "_p"), ("!", "_bang"), ("=", "_set")];
    fn is_ident_like(base: &str) -> bool {
        let mut chars = base.chars();
        matches!(chars.next(), Some(c) if c.is_alphabetic() || c == '_')
            && chars.all(|c| c.is_alphanumeric() || c == '_')
    }
    for (suffix, marker) in MARKERS {
        if let Some(base) = name.strip_suffix(suffix)
            && is_ident_like(base) {
                return Some(format!("{base}{marker}"));
            }
    }
    if is_ident_like(name) && MARKERS.iter().any(|(_, m)| name.ends_with(m)) {
        return Some(format!("{name}_"));
    }
    None
}

/// The Rust identifier for a class/module's generated struct/`mod`/`impl`
/// -- the ONE place a resolved `ClassId` becomes a Rust name.
/// A top-level class keeps the plain `safe_ident(name)`
/// (generated code for flat programs stays byte-identical); a NESTED class
/// mangles to `__c<id>_<leaf>` -- the `ClassId` makes it
/// collision-free by construction (two `Widget`s in different namespaces,
/// or a namespace path whose `_`-join would be ambiguous, can never
/// collide), the leaf keeps it readable, and the `__` prefix can't collide
/// with any top-level class (Ruby constants start with an uppercase
/// letter). The box dimension is mangled in here the same way (see below).
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
    // A reopened BUILTIN's container `pub mod` (the only
    // generated item a builtin ever gets, see `emit_builtin_reopen`) is
    // prefix-mangled: a bare `pub mod String` would shadow the prelude
    // `String` TYPE at the crate root (Rust modules and types share a
    // namespace), breaking every generated body that names `String`.
    // `Object` mangles the same way (`__bm_Object`) since top-level `def`
    // support: its methods live in a reopen-style container too, and a
    // bare `pub mod Object` would shadow `zeo_rt::Object`.
    if compiler.value_backed(cid) {
        // A per-box OVERLAY gets its own container module --
        // `__bm_b2_String` -- so a root reopen and any number of box
        // overlays of the same builtin coexist.
        // A qualified builtin name (`Enumerator::Yielder`, `File::Stat`) has
        // `::`, and `ARGF`'s class is literally named `ARGF.class` -- neither
        // `:` nor `.` is a legal Rust ident char, so flatten every non-ident
        // char to `_` so the container module is nameable (reached when a
        // module included into `Object` propagates a `__bm_` container to
        // every builtin).
        let flat: String = ci
            .name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        if ci.box_id != 0 {
            return proc_macro2::Ident::new(
                &format!("__bm_b{}_{}", ci.box_id, flat),
                proc_macro2::Span::call_site(),
            );
        }
        return proc_macro2::Ident::new(&format!("__bm_{}", flat), proc_macro2::Span::call_site());
    }
    // A class DEFINED IN a box: `__b<box>_<leaf>` -- the box id
    // disambiguates it from a same-named main-program class (the ClassId
    // isn't needed: one box defines each top-level name at most once, and
    // nested classes already took the `__c<id>_` arm above).
    if ci.box_id != 0 {
        return proc_macro2::Ident::new(
            &format!("__b{}_{}", ci.box_id, ci.name),
            proc_macro2::Span::call_site(),
        );
    }
    if RUST_PRELUDE_COLLISIONS.contains(&ci.name.as_str()) {
        return proc_macro2::Ident::new(
            &format!("__p_{}", ci.name),
            proc_macro2::Span::call_site(),
        );
    }
    safe_ident(&ci.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_name_is_itself() {
        assert_eq!(safe_ident("foo").to_string(), "foo");
        assert_eq!(safe_ident("some_method").to_string(), "some_method");
    }

    #[test]
    fn a_rust_keyword_escapes_to_a_raw_identifier() {
        // Legal Ruby names that happen to be Rust keywords -- `Ident::new`
        // panics on these, so they must become `r#`-escaped.
        for kw in ["type", "loop", "match", "fn", "let", "move", "ref", "impl"] {
            assert_eq!(safe_ident(kw).to_string(), format!("r#{kw}"));
        }
    }

    #[test]
    fn special_method_suffixes_get_plain_ascii_markers() {
        assert_eq!(safe_ident("empty?").to_string(), "empty_p");
        assert_eq!(safe_ident("save!").to_string(), "save_bang");
        assert_eq!(safe_ident("value=").to_string(), "value_set");
    }

    #[test]
    fn a_suffixed_name_whose_base_is_a_keyword_stays_unescaped() {
        // The `_p` marker already makes it a non-keyword, so no `r#`.
        assert_eq!(safe_ident("type?").to_string(), "type_p");
    }

    /// A marker is a reserved ENDING: a plain Ruby name already spelled that
    /// way must not land on the mangled form of a different method.
    /// `Gem::Resolver::InstallerSet` defines both `remote=` and `remote_set`.
    #[test]
    fn a_plain_name_ending_in_a_marker_stays_distinct_from_the_mangled_one() {
        assert_eq!(safe_ident("remote=").to_string(), "remote_set");
        assert_eq!(safe_ident("remote_set").to_string(), "remote_set_");
        assert_eq!(safe_ident("remote_set=").to_string(), "remote_set_set");
        assert_eq!(safe_ident("valid_p").to_string(), "valid_p_");
        assert_eq!(safe_ident("valid?").to_string(), "valid_p");
        assert_eq!(safe_ident("save_bang").to_string(), "save_bang_");
        assert_eq!(safe_ident("save!").to_string(), "save_bang");
        // A name merely CONTAINING a marker, or ending in a bare `_`, is
        // untouched -- nothing mangles to either shape.
        assert_eq!(safe_ident("set_remote").to_string(), "set_remote");
        assert_eq!(safe_ident("remote_").to_string(), "remote_");
    }

    #[test]
    fn operator_method_names_map_to_fixed_identifiers() {
        // `def +`/`def <=>` are ordinary method names in Ruby but aren't
        // identifier TEXT at all in Rust -- an exhaustive lookup, not an
        // escaping rule (both the `impl` and the call site route through
        // here, so they agree by construction).
        assert_eq!(safe_ident("+").to_string(), "op_add");
        assert_eq!(safe_ident("<=>").to_string(), "op_cmp");
        assert_eq!(safe_ident("[]").to_string(), "op_index");
        assert_eq!(safe_ident("[]=").to_string(), "op_index_set");
        assert_eq!(safe_ident("-@").to_string(), "op_neg");
        assert_eq!(safe_ident("==").to_string(), "op_eq");
        assert_eq!(safe_ident("===").to_string(), "op_case_eq");
    }

    #[test]
    fn an_operator_that_looks_suffixed_is_not_treated_as_a_suffixed_name() {
        // `==`/`<=`/`[]=` end in `=` but have no identifier-like base --
        // `OPERATOR_METHOD_NAMES` is consulted BEFORE the suffix rule.
        assert_eq!(safe_ident("<=").to_string(), "op_le");
        assert_eq!(safe_ident("!=").to_string(), "op_neq");
    }

    #[test]
    fn ruby_underscore_local_becomes_a_named_binding() {
        // Ruby's `_` is an ordinary readable local; Rust's `_` is not a
        // named binding (`let mut _` doesn't parse, and a macro `$x:ident`
        // matcher rejects it) -- the two corpus tests that surfaced this
        // failed inside `ruby_class!`, not at `Ident::new`.
        assert_eq!(safe_ident("_").to_string(), "__underscore");
        // Only the BARE underscore is special -- `_foo`/`__` are ordinary.
        assert_eq!(safe_ident("_foo").to_string(), "_foo");
        assert_eq!(safe_ident("__").to_string(), "__");
    }

    #[test]
    #[should_panic(expected = "internal error: `self` is a Rust path keyword")]
    fn an_unescapable_rust_path_keyword_panics_clearly() {
        // `self`/`Self`/`super`/`crate` can't be raw identifiers at all --
        // none is a legal Ruby identifier either, so reaching here is an
        // internal error worth a clear message rather than invalid output.
        safe_ident("self");
    }

    /// `class_ident` needs a real, ANALYZED `Compiler` (its arms read
    /// `lexical_parent`/`box_id`/`is_builtin`), so these go through the
    /// ordinary parse+analyze path rather than hand-building a `ClassInfo`.
    mod class_ident {
        use crate::codegen::ident::class_ident;
        use crate::compiler::{Compiler, OBJECT_CLASS};

        fn analyzed(source: &str) -> Compiler {
            let (hir, root) = crate::parse::parse_and_lower(source).expect("parses");
            crate::analyze::analyze(hir, root)
                .expect("analyzes")
                .compiler
        }

        fn ident_of(compiler: &Compiler, name: &str) -> String {
            let cid = compiler
                .resolve_class(name, &[], 0)
                .unwrap_or_else(|| panic!("no class named `{name}`"));
            class_ident(compiler, cid).to_string()
        }

        #[test]
        fn a_plain_user_class_keeps_its_own_name() {
            let compiler = analyzed("class Widget; end");
            assert_eq!(ident_of(&compiler, "Widget"), "Widget");
        }

        /// A Ruby class named after a RUST PRELUDE type would otherwise
        /// define a crate-root struct shadowing it -- and generated code
        /// plus the `ruby_class!` expansion name `Vec`/`Option`/`Box`
        /// UNQUALIFIED throughout, so the whole program stopped compiling
        /// (the real corpus symptom: `struct takes 0 generic arguments but
        /// 1 generic argument was supplied`, from a `Vec<RubyValue>` that
        /// suddenly meant the user's own `Vec`).
        #[test]
        fn a_class_named_after_a_rust_prelude_type_is_mangled() {
            for name in [
                "Vec", "Option", "Box", "Result", "Iterator", "Send", "Clone",
            ] {
                let compiler = analyzed(&format!("class {name}; end"));
                assert_eq!(
                    ident_of(&compiler, name),
                    format!("__p_{name}"),
                    "class {name}"
                );
            }
        }

        /// A Ruby BUILTIN sharing a prelude name (`String`, `Hash`) never
        /// reaches the collision rule: it takes the `__bm_` reopen arm
        /// first, which already avoids the shadowing.
        #[test]
        fn a_reopened_builtin_sharing_a_prelude_name_takes_the_reopen_arm() {
            let compiler = analyzed("class String; def shout; upcase; end; end");
            assert_eq!(ident_of(&compiler, "String"), "__bm_String");
        }

        /// Top-level `def`s live on `Object`, whose container module must
        /// not shadow `zeo_rt::Object` either.
        #[test]
        fn object_takes_the_reopen_arm_too() {
            let compiler = analyzed("def helper; 1; end");
            assert_eq!(
                class_ident(&compiler, OBJECT_CLASS).to_string(),
                "__bm_Object"
            );
        }

        /// A NESTED class mangles by ClassId, which is collision-free by
        /// construction (two `Widget`s in different namespaces can't clash).
        #[test]
        fn a_nested_class_mangles_with_its_class_id() {
            let compiler = analyzed("module Store; class Item; end; end");
            let ident = ident_of(&compiler, "Store::Item");
            assert!(
                ident.starts_with("__c") && ident.ends_with("_Item"),
                "expected a `__c<id>_Item` mangle, got `{ident}`"
            );
        }
    }
}
