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
| zlib | `zlib` | `ext-zlib` | **done** | `crc32`/`adler32`, `deflate`/`inflate`/`gzip`/`gunzip`, and the full class surface — `ZStream`/`Deflate`/`Inflate` over flate2's incremental API, `GzipFile`/`GzipWriter`/`GzipReader` over zeo's own gzip framing (flate2/miniz_oxide) |
| date | `date` | `ext-date` | **done** | `Date`/`DateTime` over an in-tree Julian-day calendar core |
| socket | `socket` | `ext-socket` | **done** | full `BasicSocket`/`IPSocket`/`TCPSocket`/`TCPServer`/`UDPSocket`/`UNIXSocket`/`UNIXServer`/`Addrinfo`/`Socket::Option` hierarchy over libc, including the non-blocking family (`accept_nonblock`/`recv_nonblock`/`connect_nonblock` and IO's `read_nonblock`/`write_nonblock`) |
| openssl | `openssl` | `ext-openssl` | **subset** | the official rust-openssl bindings over a VENDORED OpenSSL 3.x — the same EVP implementations CRuby binds. `Digest` (+ the eight algorithm subclasses), `HMAC`, `KDF.pbkdf2_hmac`/`hkdf`/`scrypt` (`PKCS5` shim), `BN` over BIGNUM, `Cipher` (CBC/CTR/ECB/OFB/CFB + the AEAD modes, with `AuthTagError`), `Random`, secure compares, version constants, and CLIENT-side `SSL::SSLContext`/`SSLSocket` over libssl (HTTPS through net/http); the exception hierarchy and `Integer#to_bn` are the gem's Ruby half. PKey generation, X509 issuance, PKCS#7, ASN1 and `SSLServer` are declined — see `docs/COMPATIBILITY.md` |
| etc | `etc` | `ext-etc` | **done** | `Etc` over libc (`getpwnam`/`getgrgid`/… + `Passwd`/`Group` structs, `sysconf`/`uname`/`nprocessors`) |
| pathname | `pathname` | `ext-pathname` | **done** | focused native `Pathname` over File/Dir |
| monitor | `monitor` | `ext-monitor` | **done** | `Monitor` + `MonitorMixin` |
| fcntl | `fcntl` | `ext-fcntl` | **done** | `Fcntl`'s `fcntl(2)`/`open(2)` flag constants, read from `libc` and `#[cfg]`'d per platform as CRuby `#ifdef`s them |
| pty | `pty` | `ext-pty` | **done** | `PTY.open`/`spawn`/`getpty`/`check` over `openpty(3)`, the child under a real controlling terminal; `ChildExited` is the gem's Ruby half, and a `check(pid, true)` raise carries only the message — its `#status` answers nil (a by-name raise can't attach one) |
| syslog | `syslog`, `syslog/logger` | `ext-syslog` | **done** | `Syslog` over `syslog(3)` — `open`/`log`/`mask` lifecycle, priority shortcuts, the full constant set, `LOG_MASK`/`LOG_UPTO`; the `Constants`/`Level`/`Option`/`Facility`/`Macros` submodules are the gem's Ruby half, and `Syslog::Logger` is vendored upstream (its extend-on-include hook is a known gap: `tests/gaps/issue_included_hook_not_fired.rb`) |
| readline | `readline` | `ext-readline` | **done** | `Readline.readline` — rustyline (pure Rust) on a terminal, a plain chomped read off `Readline.input =` or a non-tty stdin; the Enumerable `HISTORY` object, `completion_proc` wired into rustyline's completer, and the stored word-break/quote attribute surface. `VERSION` reports `"rustyline"` the way libedit builds report `"EditLine wrapper"` |
| nkf | `nkf`, `kconv` | `ext-nkf` | **subset** | `NKF.nkf`/`.guess` rebuilt over zeo's own encoding engine (which grew ISO-2022-JP and the dummy UTF-16/32 rows for it) — the conversion option subset (`-j/-e/-s/-w*`, `-J/-E/-S/-W*`, `--ic/--oc`, `-m[0]` MIME-word decode, `-x/-X` kana folding, `-Z0-2`, `-L[uwm]`), with `Kconv` the gem's vendored Ruby half. NOT nkf's whole grammar; `guess` is a reimplemented heuristic — see `docs/COMPATIBILITY.md` |
| bigdecimal | `bigdecimal`, `bigdecimal/*` | `ext-bigdecimal` | **done** | `BigDecimal` over a BigUint coefficient — bigdecimal 4.x's C slice (exact add/sub/mult, division to the documented precision rule, the rounding engine, mode/limit state, conversions, `Kernel#BigDecimal`); `**`/`power`/`sqrt`/`BigMath`/`to_d` are the gem's own Ruby, vendored in `gems/bigdecimal` and compiled like user code |
| coverage | `coverage` | `ext-coverage` | **subset** | line coverage over the AOT line instrumentation: requiring `coverage` makes the COMPILER emit per-statement hit counters plus a per-file coverable-line table, and `Coverage` replays CRuby's whole lifecycle (`start`/`setup`/`resume`/`suspend`/`result`/`peek_result`/`state`, oracle-matched errors included). A file is reported iff its top level began while measurement was set up — the entry script never is, exactly CRuby's rule. Lines only: `supported?(:branches)`/`(:methods)` answer false — see `docs/COMPATIBILITY.md` |
| TracePoint | *(core — no require)* | `ext-tracepoint` | **subset** | execution tracing over the instrumentation the runtime already carries for backtraces: `set_line` fires `:line`, `FrameGuard` push/pop fire `:call`/`:return`/`:class`/`:end` (classified by the frame label), and the raise channel fires `:raise`. `event`/`path`/`lineno`/`method_id`/`callee_id`/`defined_class`/`raised_exception`, the enable/disable lifecycle (block forms included), `TracePoint.trace`, reverse-enable-order dispatch, in-handler reentrancy suppression, and CRuby's inspect/error shapes — all oracle-matched. When nothing is enabled the hooks cost one relaxed atomic load per statement/call. `:b_call`/`:c_call`-family events, `#self`/`#binding`/`#return_value`, and the other bounds are in `docs/COMPATIBILITY.md` |

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

**The runtime object tier — `Type`, `DynamicLibrary`, `Function`,
`VariadicInvoker`, `FFI.errno`.** The same libffi machinery is also exposed
as the gem's own runtime classes, so code that treats a C call as *data*
(fiddle's pure-Ruby FFI backend is the consumer) runs unchanged:
`FFI::Type::Builtin::*` are the canonical type objects (`#size`/`#alignment`
drive fiddle's whole `SIZEOF_*`/`ALIGN_*` table); `FFI::DynamicLibrary.open`
is `dlopen(3)` (`nil` = the process image) with `#find_function` over
`dlsym`; `FFI::Function.new(ret, args, ptr_or_proc)` builds a callable
function pointer at runtime — from a code address, or from a `Proc` (a
libffi closure, so the object doubles as a C callback) — and IS an
`FFI::Pointer` (`Function < Pointer`, as in the gem);
`FFI::VariadicInvoker#call` marshals trailing `(type, value)` pairs with the
C default argument promotions. The gem's Ruby half lives in `gems/ffi/`
(`Error`/`NullPointerError`, `Platform`, `DataConverter`); every NULL
read/write through a `Pointer` raises `FFI::NullPointerError` rather than
crashing.

**fiddle rides this.** `require "fiddle"` loads `gems/fiddle/` — fiddle
1.1.8's own `lib/fiddle/ffi_backend.rb` (its JRuby/TruffleRuby path)
vendored over the tier above, plus its `closure`/`function`/`version` files
verbatim. `Fiddle.dlopen`, `Fiddle::Function`, `Fiddle::Pointer`,
`Closure::BlockCaller` callbacks (qsort works), and `TYPE_VARIADIC` calls
are all oracle-matched against the C extension (`tests/fiddle.rb`); the
`Importer` DSL (`fiddle/import`/`fiddle/struct`) is not included — see
`docs/COMPATIBILITY.md` `### fiddle` for that and the other divergences.

## Out of scope (VM internals / tooling)

`rubyvm`, `continuation`, `ripper` (we have ruby-prism),
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
