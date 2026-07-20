//! In-tree, require-gated extensions -- CRuby's `ext/` model. Each module here
//! is activated by a `require "<feature>"` the compiler recognizes as a
//! built-in feature (no filesystem file), and its class/module tables plug into
//! the same `class_table`/`class_method_table` dispatch the core builtins use.
//! Unlike the retired native-package DSL, these can construct proper
//! exceptions (they live inside `spinel-rt`).
//!
//! # A gem may also have a RUBY half
//!
//! An extension here is the NATIVE half of a gem. The gem's Ruby half, when it
//! has one, lives in `gems/<name>/lib/` and is joined to this module by the
//! feature string -- the same split CRuby makes between `archdir` (the `.so`)
//! and `rubylibdir` (the `.rb`), and the reason `digest`, `json`, `socket` and
//! `strscan` all ship both. The Ruby half pulls this one in with
//! `require "<name>.so"`, CRuby's loader idiom.
//!
//! That is where an extension's EXCEPTION classes belong. A row in the ABI
//! table is feature-gated but registers `constructor: None`; the exception
//! table is constructible but ungated and always-on -- so no row shape here is
//! both gated and constructible, and `raise_error("Foo::Error", ..)` from an
//! extension would panic. Defined in the Ruby half they are ordinary user
//! classes, registered under their fully qualified name with a real
//! constructor, and raising them by name from here works. See
//! `gems/strscan/lib/strscan.rb`.
//!
//! # Two independent gates
//!
//! An extension is behind BOTH:
//!   1. a **Ruby-level require gate** -- its constant is invisible until its
//!      `require` fires (the ABI `feature` field; see `spinel-abi`).
//!   2. a **cargo feature** (`ext-<name>` in `spinel-rt/Cargo.toml`) -- its Rust
//!      code compiles in only when that feature is on. `default`/`ext-all`
//!      enable every extension, so the common build has them all.
//!
//! # Implementation status
//!
//! Some extensions are fully implemented (real, oracle-matched methods); others
//! are **scaffolded** -- a couple of core methods, with the rest `todo!()` as a
//! compile-visible, greppable marker of exactly what's left. Scaffolding still
//! lets `require "<feature>"` succeed and the constant resolve, so downstream
//! code compiles past the `require`. See `docs/EXTENSIONS.md` for the per-
//! extension roadmap and status.
//!
//! # Adding an extension (checklist)
//!
//! 1. **ABI row** in `spinel-abi/src/lib.rs`: a `ClassId` const (next free id)
//!    and a `BUILTINS` row with `feature: Some("<require-name>")`. Ids are
//!    append-only and contiguous.
//! 2. **Module** `ext/<name>.rs` with `builtin_methods! { pub(crate) fn lookup;
//!    ... }` for instance methods and/or `pub(crate) fn lookup_class;` for
//!    class/module methods (mirror `base64.rs` for a module, `stringio.rs` for
//!    a class with instances).
//! 3. **Dispatch arms** in `builtins/mod.rs`: add a `#[cfg(feature =
//!    "ext-<name>")]` arm to `class_method_table` and/or `class_table`, plus
//!    `class_table_names` (reflection). Cfg-gate them so a feature-off build
//!    drops them cleanly.
//! 4. **Cargo feature** `ext-<name>` in `spinel-rt/Cargo.toml`, added to the
//!    `ext-all` umbrella (with `dep:` entries if it needs an optional crate).
//! 5. **Module declaration** below, cfg-gated.
//! 6. If the `require` has aliases (`cgi/util` -> `cgi/escape`), map them in
//!    `parse/loader.rs`'s `canonical_ext_feature`.

#[cfg(feature = "ext-base64")]
pub(crate) mod base64;
#[cfg(feature = "ext-cgi")]
pub(crate) mod cgi;
#[cfg(feature = "ext-date")]
pub(crate) mod date;
#[cfg(feature = "ext-digest")]
pub(crate) mod digest;
#[cfg(feature = "ext-ffi")]
pub(crate) mod ffi;
#[cfg(feature = "ext-json")]
pub(crate) mod json;
#[cfg(feature = "ext-monitor")]
pub(crate) mod monitor;
#[cfg(feature = "ext-openssl")]
pub(crate) mod openssl;
#[cfg(feature = "ext-psych")]
pub(crate) mod psych;
#[cfg(feature = "ext-socket")]
pub(crate) mod socket;
#[cfg(feature = "ext-stringio")]
pub(crate) mod stringio;
#[cfg(feature = "ext-strscan")]
pub(crate) mod strscan;
#[cfg(feature = "ext-zlib")]
pub(crate) mod zlib;
