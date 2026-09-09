# Add an extension

An *extension* is a stdlib library zeo implements in Rust: a Ruby half under
`crates/zeo-rt/ext/<name>/` beside the Rust that implements it, and a feature
flag that can leave it out.

Zeo mirrors CRuby's `ext/` model: an extension is an in-tree module that a
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
corner raises `NoMethodError` rather than panicking. Where Zeo's answer can
diverge from the upstream gem or C extension, the reason is catalogued in
[`docs/reference/compatibility.md`](../reference/compatibility.md).

The catalog below covers CRuby's full `ext/` set plus the bundled default gems
that are C-accelerated (base64, json, …). Pure-Ruby stdlib (`shellwords`,
`ostruct`, `forwardable`, …) is **not** here — it drops in through `-I` load
roots, not the `ext/` model.

## The steps

See the checklist at the top of `crates/zeo-rt/src/ext.rs`. In brief:

1. **Directory** — `crates/zeo-rt/ext/<name>/`, laid out the way a
   Rust-backed Ruby gem is: `ext/<name>/src/lib.rs` declares the class with
   the `ruby_class!` (instances) or `ruby_module!` (module functions) DSL —
   mirror `base64` (module) or `stringio` (class with instances) — and the
   Ruby half, if it has one, goes in `lib/` beside it, with a `.gemspec` at
   the root where upstream ships one.
2. **ABI row** — a `ClassId` const + `BUILTINS` row with `feature:
   Some("<require-name>")` in `crates/zeo-abi/src/lib.rs` (ids are
   append-only, contiguous).
3. **Nothing.** `ruby_class!`/`ruby_module!` export the class's table under a
   `zeo_ctable_<ID>` symbol, `build.rs` scans the runtime sources for the
   `NAME = zeo_abi::ID` spelling to list it in `CLASS_TABLE_SYMBOLS`, and
   `class_table` consults `registered_table(id)` first — so no hand-written
   dispatch arm is needed. `class_tables_are_complete` gates the list against
   `libzeo.a` in both directions, which is what stops a new class silently
   losing every method and constant at run time. One table answering for
   several ids is `alias_class_tables!`, which keeps the same spelling and so
   is picked up the same way.
4. **Cargo feature** — `ext-<name>` in `zeo-rt/Cargo.toml`, added to
   `ext-all` (with `dep:` entries if it needs an optional crate). The module
   declaration itself is generated from the directory tree, so there is no
   list to edit.
5. **Require aliases** — if the `require` has sub-spellings (`cgi/util`,
   `digest/sha2`), map them in `lower/features.rs`'s `canonical_ext_feature`.

Every method lands with a `#[cfg(test)]` unit test and (for real methods) an
oracle-matched `examples/` fixture.

## Status

| Extension | `require` | Cargo feature | Status | Notes |
|---|---|---|---|---|
| base64 | `base64` | `ext-base64` | **done** | RFC 2045/4648, urlsafe |
| stringio | `stringio` | `ext-stringio` | **done** | in-memory `StringIO` buffer (read/write/gets/puts/each_line/eof?/rewind/pos/…) |
| strscan | `strscan` | `ext-strscan` | **done** | `StringScanner` over the Regexp engine (scan/skip/match?/check/scan_until/getch/peek/rest/pre_match/…) |
| cgi (escape) | `cgi/escape`, `cgi`, `cgi/util` | `ext-cgi` | **done** | `escape`/`unescape`/`escapeHTML`/`unescapeHTML`/`escapeURIComponent` (+ `ERB::Util` routines) |
| digest | `digest`, `digest/*` | `ext-digest` | **done** | `Digest::MD5`/`SHA1`/`SHA256`/`SHA512` — class + streaming API (RustCrypto) |
| json | `json` | `ext-json` | **done** | `parse` (`symbolize_names` and the rest of json 2.21.2's option matrix), `generate`/`pretty_generate`/`dump`; both halves iterative |
| psych / yaml | `psych`, `yaml` | `ext-psych` | **done** | `load`/`safe_load`/`dump` (yaml-rust2, hand-rolled Psych block-style dump) |
| zlib | `zlib` | `ext-zlib` | **done** | `crc32`/`adler32`, `deflate`/`inflate`/`gzip`/`gunzip`, and the full class surface — `ZStream`/`Deflate`/`Inflate` over flate2's incremental API, `GzipFile`/`GzipWriter`/`GzipReader` over Zeo's own gzip framing (flate2/miniz_oxide) |
| date | `date` | `ext-date` | **done** | `Date`/`DateTime` over an in-tree Julian-day calendar core |
| socket | `socket` | `ext-socket` | **done** | full `BasicSocket`/`IPSocket`/`TCPSocket`/`TCPServer`/`UDPSocket`/`UNIXSocket`/`UNIXServer`/`Addrinfo`/`Socket::Option` hierarchy over libc, including the non-blocking family (`accept_nonblock`/`recv_nonblock`/`connect_nonblock` and IO's `read_nonblock`/`write_nonblock`) |
| openssl | `openssl` | `ext-openssl` | **subset** | the official rust-openssl bindings over a VENDORED OpenSSL 3.x — the same EVP implementations CRuby binds. `Digest` (+ the eight algorithm subclasses), `HMAC`, `KDF.pbkdf2_hmac`/`hkdf`/`scrypt` (`PKCS5` shim), `BN` over BIGNUM, `Cipher` (CBC/CTR/ECB/OFB/CFB + the AEAD modes, with `AuthTagError`), `Random`, secure compares, version constants, and CLIENT-side `SSL::SSLContext`/`SSLSocket` over libssl (HTTPS through net/http); the exception hierarchy and `Integer#to_bn` are the gem's Ruby half. PKey generation, X509 issuance, PKCS#7, ASN1 and `SSLServer` are declined — see `docs/reference/compatibility.md` |
| etc | `etc` | `ext-etc` | **done** | `Etc` over libc (`getpwnam`/`getgrgid`/… + `Passwd`/`Group` structs, `sysconf`/`uname`/`nprocessors`) |
| monitor | `monitor` | `ext-monitor` | **done** | `Monitor` + `MonitorMixin` |
| fcntl | `fcntl` | `ext-fcntl` | **done** | `Fcntl`'s `fcntl(2)`/`open(2)` flag constants, read from `libc` and `#[cfg]`'d per platform as CRuby `#ifdef`s them |
| pty | `pty` | `ext-pty` | **done** | `PTY.open`/`spawn`/`getpty`/`check` over `openpty(3)`, the child under a real controlling terminal; `ChildExited` is the gem's Ruby half, and a `check(pid, true)` raise carries only the message — its `#status` answers nil (a by-name raise can't attach one) |
| syslog | `syslog`, `syslog/logger` | `ext-syslog` | **done** | `Syslog` over `syslog(3)` — `open`/`log`/`mask` lifecycle, priority shortcuts, the full constant set, `LOG_MASK`/`LOG_UPTO`; the `Constants`/`Level`/`Option`/`Facility`/`Macros` submodules are the gem's Ruby half, and `Syslog::Logger` is vendored upstream |
| io/console | `io/console` | `ext-io-console` | **done** | the terminal modes (`raw`/`cooked`/`echo=`/`getch`/`getpass` over `termios(3)`), `winsize` over `ioctl`, the cursor escapes, and `IO::ConsoleMode`; the rows live on `IO` itself, gated on the require |
| ffi | `ffi` | `ext-ffi` | **done** | `FFI::Library`/`attach_function`, structs, pointers, callbacks and variadics over libffi; see [the FFI surface](../reference/ffi.md) |
| fiddle | `fiddle` | *(rides `ext-ffi`)* | **done** | the gem's own pure-Ruby backend over the ffi API, vendored; `Importer` is not vendored |
| nkf | `nkf`, `kconv` | `ext-nkf` | **subset** | `NKF.nkf`/`.guess` rebuilt over Zeo's own encoding engine (which grew ISO-2022-JP and the dummy UTF-16/32 rows for it) — the conversion option subset (`-j/-e/-s/-w*`, `-J/-E/-S/-W*`, `--ic/--oc`, `-m[0]` MIME-word decode, `-x/-X` kana folding, `-Z0-2`, `-L[uwm]`), with `Kconv` the gem's vendored Ruby half. NOT nkf's whole grammar; `guess` is a reimplemented heuristic — see `docs/reference/compatibility.md` |
| bigdecimal | `bigdecimal`, `bigdecimal/*` | `ext-bigdecimal` | **done** | `BigDecimal` over a BigUint coefficient — bigdecimal 4.x's C slice (exact add/sub/mult, division to the documented precision rule, the rounding engine, mode/limit state, conversions, `Kernel#BigDecimal`); `**`/`power`/`sqrt`/`BigMath`/`to_d` are the gem's own Ruby, vendored in `crates/zeo-rt/ext/bigdecimal` and compiled like user code |
| coverage | `coverage` | `ext-coverage` | **subset** | line coverage over the AOT line instrumentation: requiring `coverage` makes the COMPILER emit per-statement hit counters plus a per-file coverable-line table, and `Coverage` replays CRuby's whole lifecycle (`start`/`setup`/`resume`/`suspend`/`result`/`peek_result`/`state`, oracle-matched errors included). A file is reported iff its top level began while measurement was set up — the entry script never is, exactly CRuby's rule. Lines only: `supported?(:branches)`/`(:methods)` answer false — see `docs/reference/compatibility.md` |
| prism | `prism` | `ext-prism` | **done** | `Prism.parse`/`lex`/`parse_lex`/`parse_comments`/`dump`/`parse_success?` and their `_file` forms, over the SAME prism C library Zeo's own front end parses with. The native surface is only the `pm_serialize_*` entry points, exactly as upstream's FFI backend has it — the node classes, the visitors and the deserializer are the gem's own Ruby, vendored under `crates/zeo-rt/ext/prism/`. `Prism::Translation` (the `parser`/`ripper` adapters) is not vendored; see `crates/zeo-rt/ext/UPSTREAM.md` |
| TracePoint | *(core — no require)* | `ext-tracepoint` | **subset** | execution tracing over the instrumentation the runtime already carries for backtraces: `set_line` fires `:line`, `FrameGuard` push/pop fire `:call`/`:return`/`:class`/`:end` (classified by the frame label), and the raise channel fires `:raise`. `event`/`path`/`lineno`/`method_id`/`callee_id`/`defined_class`/`raised_exception`, the enable/disable lifecycle (block forms included), `TracePoint.trace`, reverse-enable-order dispatch, in-handler reentrancy suppression, and CRuby's inspect/error shapes — all oracle-matched. When nothing is enabled the hooks cost one relaxed atomic load per statement/call. `:b_call`/`:c_call`-family events, `#self`/`#binding`/`#return_value`, and the other bounds are in `docs/reference/compatibility.md` |

`require "io/wait"` (`IO#wait_readable`/`#wait_writable` over real `poll(2)`)
is pure ceremony: the rows are unconditional on the `IO` table. `Pathname` is
a core class with no require, as in ruby 4.0. `objspace` is the same shape:
`ObjectSpace` is a live builtin, so `memsize_of`/`reachable_objects_from`/`count_symbols` answer
without the require (`crates/zeo-rt/src/builtins/objspace.rs`; what it declines
and why is in [`docs/reference/compatibility.md`](../reference/compatibility.md)). `ARGF` is a live
builtin (`zeo_abi::ARGF_CLASS`), and `rbconfig` resolves through a synthetic
shim (`crates/zeo/src/parse/shims/rbconfig.rb.in`, rendered by build.rs from
the build target's platform facts and spliced by
`parse/loader/splice.rs::splice_synthetic_shim`). The shim's remaining
limitation is a synthesized FHS install prefix, not a real install layout.

No extension is catalogued and waiting: every one ruby 4.0.6 ships either has
a module here or is out of scope below.

## Out of scope (VM internals / tooling)

`rubyvm`, `continuation`, `ripper`, `win32`, `-test-`. `require`ing one raises
the normal `cannot load such file` — except where Zeo has a decision to state,
in which case the message carries it (`zeo_abi::declined_feature_reason`).
`ripper` is the case today: it points at `require "prism"`, the same parser
Zeo's own front end runs, which answers with a syntax tree rather than
parse.y's reduction stream. See "Declined" in
[`docs/reference/compatibility.md`](../reference/compatibility.md).
