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

use proc_macro2::{Ident, Span};

const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "static", "struct", "trait", "true", "type", "unsafe", "use", "where",
    "while", "abstract", "become", "box", "do", "final", "macro", "override", "priv", "typeof",
    "unsized", "virtual", "yield", "try",
];

const UNESCAPABLE: &[&str] = &["self", "Self", "super", "crate"];

pub fn safe_ident(name: &str) -> Ident {
    if UNESCAPABLE.contains(&name) {
        panic!("`{name}` is not a valid Ruby identifier and can't be escaped as a Rust one");
    }
    if RUST_KEYWORDS.contains(&name) {
        Ident::new_raw(name, Span::call_site())
    } else {
        Ident::new(name, Span::call_site())
    }
}
