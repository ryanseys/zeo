# Extensions (`ext/`)

spinel-rs mirrors CRuby's `ext/` model: an extension is an in-tree module that a
`require` activates. Each one is behind **two independent gates**:

1. **Ruby require gate** — its constant is invisible until its `require` fires
   (the ABI `feature` field). Referencing it un-`require`d is a `NameError`,
   exactly as in CRuby.
2. **Cargo feature** — `ext-<name>` in `crates/spinel-rt/Cargo.toml`. Its Rust
   code compiles in only when the feature is on. `default` = `ext-all`, so an
   out-of-the-box build has every extension and `require`ing one just works.
   Slim a binary with `--no-default-features --features ext-json,ext-stringio,…`.

**Implementation status.** Some extensions carry real, oracle-matched methods;
others are **scaffolded** — a couple of core methods, with the rest `todo!()` as
a compile-visible, greppable marker (`rg 'todo!' crates/spinel-rt/src/ext`).
Scaffolding still lets `require` succeed and the constant resolve, so downstream
code compiles *past* the `require`; only the unbuilt method call panics.

The catalog below covers CRuby's full `ext/` set plus the bundled default gems
that are C-accelerated (base64, json, …). Pure-Ruby stdlib (`shellwords`,
`ostruct`, `forwardable`, …) is **not** here — it drops in through `-I` load
roots (see `cargo xtask stdlib-status`), not the `ext/` model.

## Status

| Extension | `require` | Cargo feature | Status | Notes |
|---|---|---|---|---|
| base64 | `base64` | `ext-base64` | **done** | RFC 2045/4648, urlsafe |
| stringio | `stringio` | `ext-stringio` | **done** | in-memory IO buffer (read/write/gets/puts/each_line/eof?/rewind/pos/…); `seek`/`getc`/`readline`/`truncate` `todo!` |
| strscan | `strscan` | `ext-strscan` | **done** | `StringScanner` over the Regexp engine (scan/skip/match?/check/scan_until/getch/peek/rest/pre_match/…); `exist?`/`unscan`/`get_byte` `todo!` |
| cgi (escape) | `cgi/escape`, `cgi`, `cgi/util` | `ext-cgi` | **done** | `escape`/`unescape`/`escapeHTML`/`unescapeHTML`/`escapeURIComponent` (+ `ERB::Util` routines) |
| digest | `digest`, `digest/*` | `ext-digest` | **done** | `Digest::MD5`/`SHA1`/`SHA256`/`SHA512` — class + streaming API (RustCrypto); `Digest.hexencode`/`bubblebabble`/`digest_length` `todo!` |
| json | `json` | `ext-json` | **done** | `parse` (serde_json, `symbolize_names`), `generate`/`pretty_generate`/`dump`. `to_json` monkeypatch + `JSON::ParserError` class are gaps |
| psych / yaml | `psych`, `yaml` | `ext-psych` | **done** | `load`/`safe_load`/`dump` (yaml-rust2, hand-rolled Psych block-style dump). `parse` (node tree)/`load_file` `todo!`; aliases/anchors load as nil |
| zlib | `zlib` | `ext-zlib` | **partial** | `crc32`/`adler32` real; `deflate`/`inflate`/`Gzip*` `todo!` (pending a `flate2` backend) |
| date | `date` | `ext-date` | **scaffolded** | `Date`/`DateTime` constants resolve; all methods `todo!` (needs a Julian-day calendar core) |
| socket | `socket` | `ext-socket` | **scaffolded** | `Socket` constant resolves; all methods `todo!` (needs `libc`/`std::net` + the `BasicSocket`/`TCPSocket`/`UDPSocket` hierarchy) |
| openssl | `openssl` | `ext-openssl` | **scaffolded** | `OpenSSL` constant resolves; all methods `todo!`. Digest/HMAC could reuse RustCrypto; TLS/PKey needs FFI or rustls |

## Deferred (catalogued, no module yet)

These extend the **IO core** rather than adding a new class, so they wait on the
builtin-reopen mechanism (adding instance methods to a required builtin):

| Extension | `require` | Why deferred |
|---|---|---|
| io/wait | `io/wait` | adds `IO#wait_readable`/`#wait_writable` (needs `libc::poll` + builtin reopen) |
| io/console | `io/console` | adds `IO#raw`/`#getch`/`#winsize` (termios) |
| io/nonblock | `io/nonblock` | adds `IO#nonblock` |
| ARGF | (core) | ARGV-consuming stream over the IO core |
| fcntl / etc / rbconfig | `fcntl`/`etc` | constant-only modules — need the module-constant exposure seam, not method tables |

## FFI — the real `ffi` gem, AOT-compiled (#204)

spinel implements the **real `ffi` gem API**, not a custom DSL, so a program
using it runs identically under CRuby+ffi and spinel (the north star). `require
"ffi"` is a native no-op; `extend FFI::Library` marks a module; `ffi_lib` and
`attach_function` are recognized at **compile time** and emit a fn-local
`extern "C"` declaration with `#[link(name = ..)]` plus a wrapper method that
marshals `RubyValue`↔C — so the C function is called directly, no libffi and no
`dlopen`. Both the plain `attach_function :name, [args], ret` and the 4-arg
rename form `:ruby_name, :c_name, [args], ret` are supported.

**Types (scalar slice):** `:void`, the integer family (`:char`/`:short`/`:int`/
`:long` and the fixed-width `:int8`…`:int64`), their unsigned twins and
`:size_t`, `:float`/`:double`, `:bool`, and `:string` (a `const char *` — a
NUL-terminated copy in, a Ruby String out). A wrong argument type is a
`TypeError`, exactly as the gem raises. Verified byte-for-byte against
`ffi 1.17.4` — see `examples/ffi_libc.rb`.

**Follow-ons:** `FFI::Pointer`/`FFI::MemoryPointer`, `FFI::Struct` + `layout`,
`callback`, enums, `typedef`, and varargs. The native-gem list in `cargo xtask
gem-compat` is the broader work-list this escape hatch serves.

## Out of scope (VM internals / tooling)

`objspace`, `rubyvm`, `coverage`, `continuation`, `ripper` (we have ruby-prism),
`pty`, `win32`, `-test-`. `require`ing one is the normal `cannot load such file`.

## Adding an extension

See the checklist at the top of `crates/spinel-rt/src/ext/mod.rs`. In brief:

1. **ABI row** — a `ClassId` const + `BUILTINS` row with `feature:
   Some("<require-name>")` in `crates/spinel-abi/src/lib.rs` (ids are
   append-only, contiguous).
2. **Module** — `crates/spinel-rt/src/ext/<name>.rs` with `builtin_methods! {
   pub(crate) fn lookup; … }` (instance methods) and/or `pub(crate) fn
   lookup_class;` (class/module methods). Mirror `base64.rs` (module) or
   `stringio.rs` (class with instances).
3. **Dispatch arms** — cfg-gated arms in `builtins/mod.rs`'s `class_method_table`
   / `class_table` / `class_table_names`.
4. **Cargo feature** — `ext-<name>` in `spinel-rt/Cargo.toml`, added to
   `ext-all` (with `dep:` entries if it needs an optional crate).
5. **Module declaration** — cfg-gated `pub(crate) mod <name>;` in `ext/mod.rs`.
6. **Require aliases** — if the `require` has sub-spellings (`cgi/util`,
   `digest/sha2`), map them in `parse/loader.rs`'s `canonical_ext_feature`.

Every method lands with a `#[cfg(test)]` unit test and (for real methods) an
oracle-matched `examples/` fixture.
