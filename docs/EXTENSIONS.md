# Extensions (`ext/`)

zeo mirrors CRuby's `ext/` model: an extension is an in-tree module that a
`require` activates. Each one is behind **two independent gates**:

1. **Ruby require gate** — its constant is invisible until its `require` fires
   (the ABI `feature` field). Referencing it un-`require`d is a `NameError`,
   exactly as in CRuby.
2. **Cargo feature** — `ext-<name>` in `crates/zeo-rt/Cargo.toml`. Its Rust
   code compiles in only when the feature is on. `default` = `ext-all`, so an
   out-of-the-box build has every extension and `require`ing one just works.
   Slim a binary with `--no-default-features --features ext-json,ext-stringio,…`.

**Implementation status.** Every extension module carries real, oracle-matched
methods — none are `todo!()` scaffolds. A few provide a deliberate *subset* of
their upstream's surface (noted per row below); a call into an unimplemented
corner raises `NoMethodError` rather than panicking. Where zeo's answer can
diverge from the upstream gem or C extension, the reason is catalogued in
[`docs/COMPATIBILITY.md`](COMPATIBILITY.md).

The catalog below covers CRuby's full `ext/` set plus the bundled default gems
that are C-accelerated (base64, json, …). Pure-Ruby stdlib (`shellwords`,
`ostruct`, `forwardable`, …) is **not** here — it drops in through `-I` load
roots (see `cargo xtask stdlib-status`), not the `ext/` model.

## Status

| Extension | `require` | Cargo feature | Status | Notes |
|---|---|---|---|---|
| base64 | `base64` | `ext-base64` | **done** | RFC 2045/4648, urlsafe |
| stringio | `stringio` | `ext-stringio` | **done** | in-memory `StringIO` buffer (read/write/gets/puts/each_line/eof?/rewind/pos/…) |
| strscan | `strscan` | `ext-strscan` | **done** | `StringScanner` over the Regexp engine (scan/skip/match?/check/scan_until/getch/peek/rest/pre_match/…) |
| cgi (escape) | `cgi/escape`, `cgi`, `cgi/util` | `ext-cgi` | **done** | `escape`/`unescape`/`escapeHTML`/`unescapeHTML`/`escapeURIComponent` (+ `ERB::Util` routines) |
| digest | `digest`, `digest/*` | `ext-digest` | **done** | `Digest::MD5`/`SHA1`/`SHA256`/`SHA512` — class + streaming API (RustCrypto) |
| json | `json` | `ext-json` | **done** | `parse` (serde_json, `symbolize_names`), `generate`/`pretty_generate`/`dump` |
| psych / yaml | `psych`, `yaml` | `ext-psych` | **done** | `load`/`safe_load`/`dump` (yaml-rust2, hand-rolled Psych block-style dump) |
| zlib | `zlib` | `ext-zlib` | **done** | `crc32`/`adler32` plus `deflate`/`inflate`/`gzip`/`gunzip` (flate2/miniz_oxide) |
| date | `date` | `ext-date` | **done** | `Date`/`DateTime` over an in-tree Julian-day calendar core |
| socket | `socket` | `ext-socket` | **done** | full `BasicSocket`/`IPSocket`/`TCPSocket`/`TCPServer`/`UDPSocket`/`UNIXSocket`/`UNIXServer`/`Addrinfo` hierarchy over libc |
| openssl | `openssl` | `ext-openssl` | **subset** | `OpenSSL::Random` bytes + fixed-length secure compare; `Cipher`/`PKey`/`SSL` still need an FFI or rustls backend |
| etc | `etc` | `ext-etc` | **done** | `Etc` over libc (`getpwnam`/`getgrgid`/… + `Passwd`/`Group` structs, `sysconf`/`uname`/`nprocessors`) |
| pathname | `pathname` | `ext-pathname` | **done** | focused native `Pathname` over File/Dir |
| monitor | `monitor` | `ext-monitor` | **done** | `Monitor` + `MonitorMixin` |
| fcntl | `fcntl` | `ext-fcntl` | **done** | `Fcntl`'s `fcntl(2)`/`open(2)` flag constants, read from `libc` and `#[cfg]`'d per platform as CRuby `#ifdef`s them |

The IO-core extensions have landed as unconditional rows on the `IO` table:
`require "io/wait"` (`IO#wait_readable`/`#wait_writable` over real `poll(2)`)
and `require "io/console"` (the terminal modes — `raw`/`cooked`/`echo=`/
`getch`/`getpass` over `termios(3)`, `winsize` over `ioctl`, and the cursor
escapes; `crates/zeo-rt/src/builtins/io_console.rs`) are pure ceremony — the
methods are always present. `objspace` is the same shape: `ObjectSpace` is a
live builtin, so `memsize_of`/`reachable_objects_from`/`count_symbols` answer
without the require (`crates/zeo-rt/src/builtins/objspace.rs`; what it declines
and why is in [`docs/COMPATIBILITY.md`](COMPATIBILITY.md)). `ARGF` is a live
builtin (`zeo_abi::ARGF_CLASS`), and `rbconfig` resolves through a synthetic
shim (see `docs/todo/bundler-northstar.md`).

## Deferred (catalogued, no module yet)

| Extension | `require` | Why deferred |
|---|---|---|
| _(none currently)_ | | |

## FFI — the real `ffi` gem, AOT-compiled

zeo implements the **real `ffi` gem API**, not a custom DSL, so a program
using it runs identically under CRuby+ffi and zeo (the north star). `require
"ffi"` is a native no-op; `extend FFI::Library` marks a module; `ffi_lib` and
`attach_function` are recognized at **compile time** and emit a fn-local
`extern "C"` declaration with `#[link(name = ..)]` plus a wrapper method that
marshals `RubyValue`↔C — so the C function is called directly, no libffi and no
`dlopen`. Both the plain `attach_function :name, [args], ret` and the 4-arg
rename form `:ruby_name, :c_name, [args], ret` are supported.

**Types:** `:void`, the integer family (`:char`/`:short`/`:int`/`:long` and the
fixed-width `:int8`…`:int64`), their unsigned twins and `:size_t`,
`:float`/`:double`, `:bool`, `:string` (a `const char *` — a NUL-terminated copy
in, a Ruby String out), and `:pointer` (below). A wrong argument type is a
`TypeError`, exactly as the gem raises. Verified byte-for-byte against
`ffi 1.17.4` — see `tests/ffi_libc.rb`, `tests/ffi_memory.rb`,
`tests/ffi_struct.rb`.

**Memory — `FFI::Pointer` / `FFI::MemoryPointer`.** Real runtime classes over a
native heap buffer. `MemoryPointer.new(:int, 3)` / `.new(bytes)` /
`.from_string(s)` allocate; the full typed accessor surface is present —
`read_/write_` (offset 0) and `get_/put_` (at an offset) for `int8`…`int64`,
their unsigned twins, `float`/`double`, `pointer`, `string`, and raw `bytes`,
plus `read_array_of_int`/`write_array_of_int` (& `long`/`double`), pointer
arithmetic (`ptr + n`), `null?`, `address`, `size`. Bounds-checked (`IndexError`
past an owned buffer). A `:pointer` argument passes the raw address; a `:pointer`
return wraps back to an `FFI::Pointer`.

**`typedef` / `enum`.** `typedef :existing, :alias` (compile-time type alias);
`enum :tag, [:a, 0, :b, ...]` as an `attach_function` type — a Symbol marshals to
its int, an int return maps back to its Symbol (unmapped → Integer), with
auto-increment.

**`FFI::Struct` + `layout`.** `class T < FFI::Struct; layout :f, :type, …; end`.
The `layout` is recognized at compile time; `[]`/`[]=`/`size`/`offset_of`/
`members` are synthesized over an owned `FFI::MemoryPointer` with C field offsets
and alignment. A struct auto-converts to its pointer when passed to a C
`:pointer` argument.

**`callback` and `varargs`.** Both are implemented over a *runtime* C-call
builder (libffi, the `ext-ffi` cargo feature). `callback :tag, [args], ret`
registers a C function-pointer type, so a Ruby `Proc` passed for a `:tag`
argument is marshaled into a libffi closure; a `:varargs` marker in an
`attach_function` type list (`[:string, :varargs]`) builds the variadic call
interface per runtime call, since its trailing arity/types aren't known at
compile time. These are the one part of the gem that the pure
compile-time-`extern` model can't reach without libffi, hence the extra dep.

## Out of scope (VM internals / tooling)

`rubyvm`, `coverage`, `continuation`, `ripper` (we have ruby-prism), `pty`,
`win32`, `-test-`. `require`ing one is the normal `cannot load such file`.

## Adding an extension

See the checklist at the top of `crates/zeo-rt/src/ext/mod.rs`. In brief:

1. **ABI row** — a `ClassId` const + `BUILTINS` row with `feature:
   Some("<require-name>")` in `crates/zeo-abi/src/lib.rs` (ids are
   append-only, contiguous).
2. **Module** — `crates/zeo-rt/src/ext/<name>.rs` declaring its class with the
   `ruby_class!` (instances) or `ruby_module!` (module functions) DSL. Mirror
   `base64.rs` (module) or `stringio.rs` (class with instances).
3. **Dispatch arms** — cfg-gated arms in `builtins/mod.rs`'s `class_method_table`
   / `class_table` / `class_table_names`.
4. **Cargo feature** — `ext-<name>` in `zeo-rt/Cargo.toml`, added to
   `ext-all` (with `dep:` entries if it needs an optional crate).
5. **Module declaration** — cfg-gated `pub(crate) mod <name>;` in `ext/mod.rs`.
6. **Require aliases** — if the `require` has sub-spellings (`cgi/util`,
   `digest/sha2`), map them in `parse/loader.rs`'s `canonical_ext_feature`.

Every method lands with a `#[cfg(test)]` unit test and (for real methods) an
oracle-matched `examples/` fixture.
