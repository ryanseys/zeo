//! In-tree, require-gated extensions -- CRuby's `ext/` model. Each module here
//! is activated by a `require "<feature>"` the compiler recognizes as a
//! built-in feature (no filesystem file), and its class/module tables plug into
//! the same `class_table`/`class_method_table` dispatch the core builtins use.
//! Living inside `zeo-rt`, they can construct proper exceptions.
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
//!      `require` fires (the ABI `feature` field; see `zeo-abi`).
//!   2. a **cargo feature** (`ext-<name>` in `zeo-rt/Cargo.toml`) -- its Rust
//!      code compiles in only when that feature is on. `default`/`ext-all`
//!      enable every extension, so the common build has them all.
//!
//! # Implementation status
//!
//! Every method here is real and oracle-matched -- no `todo!()` scaffolds. A
//! few extensions ship a deliberate SUBSET of their upstream surface and raise
//! `NoMethodError` at the edges rather than pretending; `docs/EXTENSIONS.md`
//! names which, and what each leaves out.
//!
//! # Adding an extension (checklist)
//!
//! 1. **ABI row** in `zeo-abi/src/lib.rs`: a `ClassId` const (next free id)
//!    and a `BUILTINS` row with `feature: Some("<require-name>")`. Ids are
//!    append-only and contiguous.
//! 2. **Module** `ext/<name>.rs` declaring its class with the `ruby_class!`
//!    (instances) or `ruby_module!` (module functions) DSL (mirror `base64.rs`
//!    for a module, `stringio.rs` for a class with instances).
//! 3. **Nothing.** `ruby_class!`/`ruby_module!` export the table under a
//!    `zeo_ctable_<ID>` symbol that build.rs lists in `CLASS_TABLE_SYMBOLS`,
//!    and `class_table` consults `registered_table(id)` first, so no
//!    hand-written dispatch arm is needed. One table answering for several
//!    ids is `alias_class_tables!`, which keeps the same spelling and so is
//!    listed the same way.
//! 4. **Cargo feature** `ext-<name>` in `zeo-rt/Cargo.toml`, added to the
//!    `ext-all` umbrella (with `dep:` entries if it needs an optional crate).
//! 5. **Module declaration** below, cfg-gated.
//! 6. If the `require` has aliases (`cgi/util` -> `cgi/escape`), map them in
//!    `parse/loader.rs`'s `canonical_ext_feature`.

#[cfg(feature = "ext-base64")]
pub(crate) mod base64;
#[cfg(feature = "ext-bigdecimal")]
pub(crate) mod bigdecimal;
#[cfg(feature = "ext-cgi")]
pub(crate) mod cgi;
#[cfg(feature = "ext-coverage")]
pub(crate) mod coverage;
#[cfg(feature = "ext-date")]
pub(crate) mod date;
#[cfg(feature = "ext-digest")]
pub(crate) mod digest;
#[cfg(feature = "ext-etc")]
pub(crate) mod etc;
#[cfg(feature = "ext-fcntl")]
pub(crate) mod fcntl;
#[cfg(feature = "ext-ffi")]
pub(crate) mod ffi;
#[cfg(feature = "ext-json")]
pub(crate) mod json;
#[cfg(feature = "ext-monitor")]
pub(crate) mod monitor;
#[cfg(feature = "ext-nkf")]
pub(crate) mod nkf;
#[cfg(feature = "ext-openssl")]
pub(crate) mod openssl;
#[cfg(feature = "ext-prism")]
pub(crate) mod prism;
#[cfg(feature = "ext-psych")]
pub(crate) mod psych;
#[cfg(feature = "ext-pty")]
pub(crate) mod pty;
#[cfg(feature = "ext-readline")]
pub(crate) mod readline;
#[cfg(feature = "ext-socket")]
pub(crate) mod socket;
#[cfg(feature = "ext-stringio")]
pub(crate) mod stringio;
#[cfg(feature = "ext-strscan")]
pub(crate) mod strscan;
#[cfg(feature = "ext-syslog")]
pub(crate) mod syslog;
#[cfg(feature = "ext-tracepoint")]
pub(crate) mod tracepoint;
#[cfg(feature = "ext-zlib")]
pub(crate) mod zlib;
