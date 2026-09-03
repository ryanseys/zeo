//! In-tree, require-gated extensions -- CRuby's `ext/` model. Each module here
//! is activated by a `require "<feature>"` the compiler recognizes as a
//! built-in feature (no filesystem file), and its class/module tables plug into
//! the same `class_table`/`class_method_table` dispatch the core builtins use.
//! Living inside `zeo-rt`, they can construct proper exceptions.
//!
//! # One directory per library
//!
//! The code lives in `crates/zeo-rt/ext/`, one directory per library, each
//! laid out the way a Rust-backed Ruby gem is (`rb-sys`'s own template, and
//! `commonmarker`):
//!
//! ```text
//! ext/json/
//!   json.gemspec          where upstream ships one
//!   lib/json.rb           the Ruby half
//!   ext/json/src/*.rs     zeo's Rust, where a Rust gem puts its Rust
//! ```
//!
//! So a library is self-contained and could be lifted out as a gem of its
//! own. `ext/<name>/src/` holds the NATIVE half -- what CRuby writes in C --
//! and `lib/` the Ruby half, the same split CRuby makes between `archdir`
//! (the `.so`) and `rubylibdir` (the `.rb`). The Ruby half pulls the native
//! one in with `require "<name>.so"`, CRuby's loader idiom.
//!
//! A `.gemspec` goes there only where ruby ships one. `socket`, `pty` and
//! `monitor` are library files rather than gems, and the compiler discovers
//! them by their `lib/` alone. An upstream gem's own Ruby (`nkf`'s
//! `kconv.rb`, `syslog`'s `logger.rb`) rides in that `lib/` too, because one
//! name resolves to one directory. `fiddle` is pure Ruby over zeo's ffi and
//! so has no `ext/fiddle/src/` at all.
//!
//! That is where an extension's EXCEPTION classes belong. A row in the ABI
//! table is feature-gated but registers `constructor: None`; the exception
//! table is constructible but ungated and always-on -- so no row shape here is
//! both gated and constructible, and `raise_error("Foo::Error", ..)` from an
//! extension would panic. Defined in the Ruby half they are ordinary user
//! classes, registered under their fully qualified name with a real
//! constructor, and raising them by name from here works. See
//! `ext/strscan/lib/strscan.rb`.
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
//! `NoMethodError` at the edges rather than pretending; `docs/how-to/add-an-extension.md`
//! names which, and what each leaves out.
//!
//! # Adding an extension (checklist)
//!
//! 1. **Directory** `ext/<name>/ext/<name>/src/lib.rs` declaring its class
//!    with the `ruby_class!` (instances) or `ruby_module!` (module functions)
//!    DSL -- mirror `base64` for a module, `stringio` for a class with
//!    instances. Its Ruby half, if it has one, goes in `ext/<name>/lib/`.
//! 2. **ABI row** in `zeo-abi/src/lib.rs`: a `ClassId` const (next free id)
//!    and a `BUILTINS` row with `feature: Some("<require-name>")`. Ids are
//!    append-only and contiguous.
//! 3. **Cargo feature** `ext-<name>` in `zeo-rt/Cargo.toml`, added to the
//!    `ext-all` umbrella (with `dep:` entries if it needs an optional crate).
//! 4. **Nothing else.** The module declaration below is generated from the
//!    directory tree, and `ruby_class!`/`ruby_module!` export the table under
//!    a `zeo_ctable_<ID>` symbol that zeo's build.rs lists in
//!    `CLASS_TABLE_SYMBOLS`, which `class_table` consults through
//!    `registered_table(id)` -- so no hand-written dispatch arm either. One
//!    table answering for several ids is `alias_class_tables!`.
//! 5. If the `require` has aliases (`cgi/util` -> `cgi/escape`), map them in
//!    `parse/loader.rs`'s `canonical_ext_feature`.

// One `pub(crate) mod` per extension this build compiles, from the `ext/`
// tree -- see `build.rs`'s `declare_exts`.
include!(concat!(env!("OUT_DIR"), "/ext_mods.rs"));
