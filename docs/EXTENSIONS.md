# Extensions (`ext/`)

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
[`docs/COMPATIBILITY.md`](COMPATIBILITY.md).

The catalog below covers CRuby's full `ext/` set plus the bundled default gems
that are C-accelerated (base64, json, …). Pure-Ruby stdlib (`shellwords`,
`ostruct`, `forwardable`, …) is **not** here — it drops in through `-I` load
roots, not the `ext/` model.

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
| zlib | `zlib` | `ext-zlib` | **done** | `crc32`/`adler32`, `deflate`/`inflate`/`gzip`/`gunzip`, and the full class surface — `ZStream`/`Deflate`/`Inflate` over flate2's incremental API, `GzipFile`/`GzipWriter`/`GzipReader` over Zeo's own gzip framing (flate2/miniz_oxide) |
| date | `date` | `ext-date` | **done** | `Date`/`DateTime` over an in-tree Julian-day calendar core |
| socket | `socket` | `ext-socket` | **done** | full `BasicSocket`/`IPSocket`/`TCPSocket`/`TCPServer`/`UDPSocket`/`UNIXSocket`/`UNIXServer`/`Addrinfo`/`Socket::Option` hierarchy over libc, including the non-blocking family (`accept_nonblock`/`recv_nonblock`/`connect_nonblock` and IO's `read_nonblock`/`write_nonblock`) |
| openssl | `openssl` | `ext-openssl` | **subset** | the official rust-openssl bindings over a VENDORED OpenSSL 3.x — the same EVP implementations CRuby binds. `Digest` (+ the eight algorithm subclasses), `HMAC`, `KDF.pbkdf2_hmac`/`hkdf`/`scrypt` (`PKCS5` shim), `BN` over BIGNUM, `Cipher` (CBC/CTR/ECB/OFB/CFB + the AEAD modes, with `AuthTagError`), `Random`, secure compares, version constants, and CLIENT-side `SSL::SSLContext`/`SSLSocket` over libssl (HTTPS through net/http); the exception hierarchy and `Integer#to_bn` are the gem's Ruby half. PKey generation, X509 issuance, PKCS#7, ASN1 and `SSLServer` are declined — see `docs/COMPATIBILITY.md` |
| etc | `etc` | `ext-etc` | **done** | `Etc` over libc (`getpwnam`/`getgrgid`/… + `Passwd`/`Group` structs, `sysconf`/`uname`/`nprocessors`) |
| pathname | `pathname` | `ext-pathname` | **done** | focused native `Pathname` over File/Dir |
| monitor | `monitor` | `ext-monitor` | **done** | `Monitor` + `MonitorMixin` |
| fcntl | `fcntl` | `ext-fcntl` | **done** | `Fcntl`'s `fcntl(2)`/`open(2)` flag constants, read from `libc` and `#[cfg]`'d per platform as CRuby `#ifdef`s them |
| pty | `pty` | `ext-pty` | **done** | `PTY.open`/`spawn`/`getpty`/`check` over `openpty(3)`, the child under a real controlling terminal; `ChildExited` is the gem's Ruby half, and a `check(pid, true)` raise carries only the message — its `#status` answers nil (a by-name raise can't attach one) |
| syslog | `syslog`, `syslog/logger` | `ext-syslog` | **done** | `Syslog` over `syslog(3)` — `open`/`log`/`mask` lifecycle, priority shortcuts, the full constant set, `LOG_MASK`/`LOG_UPTO`; the `Constants`/`Level`/`Option`/`Facility`/`Macros` submodules are the gem's Ruby half, and `Syslog::Logger` is vendored upstream |
| readline | `readline` | `ext-readline` | **done** | `Readline.readline` — rustyline (pure Rust) on a terminal, a plain chomped read off `Readline.input =` or a non-tty stdin; the Enumerable `HISTORY` object, `completion_proc` wired into rustyline's completer, and the stored word-break/quote attribute surface. `VERSION` reports `"rustyline"` the way libedit builds report `"EditLine wrapper"` |
| nkf | `nkf`, `kconv` | `ext-nkf` | **subset** | `NKF.nkf`/`.guess` rebuilt over Zeo's own encoding engine (which grew ISO-2022-JP and the dummy UTF-16/32 rows for it) — the conversion option subset (`-j/-e/-s/-w*`, `-J/-E/-S/-W*`, `--ic/--oc`, `-m[0]` MIME-word decode, `-x/-X` kana folding, `-Z0-2`, `-L[uwm]`), with `Kconv` the gem's vendored Ruby half. NOT nkf's whole grammar; `guess` is a reimplemented heuristic — see `docs/COMPATIBILITY.md` |
| bigdecimal | `bigdecimal`, `bigdecimal/*` | `ext-bigdecimal` | **done** | `BigDecimal` over a BigUint coefficient — bigdecimal 4.x's C slice (exact add/sub/mult, division to the documented precision rule, the rounding engine, mode/limit state, conversions, `Kernel#BigDecimal`); `**`/`power`/`sqrt`/`BigMath`/`to_d` are the gem's own Ruby, vendored in `crates/zeo-rt/ext/bigdecimal` and compiled like user code |
| coverage | `coverage` | `ext-coverage` | **subset** | line coverage over the AOT line instrumentation: requiring `coverage` makes the COMPILER emit per-statement hit counters plus a per-file coverable-line table, and `Coverage` replays CRuby's whole lifecycle (`start`/`setup`/`resume`/`suspend`/`result`/`peek_result`/`state`, oracle-matched errors included). A file is reported iff its top level began while measurement was set up — the entry script never is, exactly CRuby's rule. Lines only: `supported?(:branches)`/`(:methods)` answer false — see `docs/COMPATIBILITY.md` |
| prism | `prism` | `ext-prism` | **done** | `Prism.parse`/`lex`/`parse_lex`/`parse_comments`/`dump`/`parse_success?` and their `_file` forms, over the SAME prism C library Zeo's own front end parses with. The native surface is only the `pm_serialize_*` entry points, exactly as upstream's FFI backend has it — the node classes, the visitors and the deserializer are the gem's own Ruby, vendored under `crates/zeo-rt/ext/prism/`. `Prism::Translation` (the `parser`/`ripper` adapters) is not vendored; see `gems/UPSTREAM.md` |
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
shim (`crates/zeo/src/parse/shims/rbconfig.rb.in`, rendered by build.rs from
the build target's platform facts and spliced by
`parse/loader/splice.rs::splice_synthetic_shim`). The shim's remaining
limitation is a synthesized FHS install prefix, not a real install layout.

## Deferred (catalogued, no module yet)

| Extension | `require` | Why deferred |
|---|---|---|
| _(none currently)_ | | |

## A gem's own C extension

A gem that ships its C as SOURCE is compiled from it. zeo is
**source-compatible with CRuby and ABI-incompatible**: a gem's `ext/**/*.c`
builds against MRI's own headers (`crates/zeo-rt/cext/include/`, vendored
verbatim from `ruby/ruby` at the pinned tag plus one patch), and a prebuilt
MRI `.so` never loads.

The whole path, per gem:

1. The gemspec's `s.extensions` names an `extconf.rb`.
2. zeo runs it, with `RbConfig::CONFIG` supplied by its own shim and `mkmf`
   vendored from the same pin as the headers. mkmf writes a Makefile.
3. zeo reads that Makefile's variables, then compiles and links from them --
   in parallel and without `make`. A Makefile carrying a rule mkmf did not
   write (a `depend` file, a generated header) goes to real `make` instead;
   `ZEO_CEXT_MAKE=1` forces that path for everything.
4. The program `dlopen`s the result at the `require`'s own line and calls
   `Init_<name>`.

The build is **out of tree**. A gem store is shared and often read only, so
the sources are staged into zeo's cache and built there, keyed by their own
bytes -- a second compile of the same gem finds the shared object and skips
the build.

### What this does not do

* **A precompiled binary gem never loads.** `nokogiri-1.16.0-arm64-darwin`
  ships a `.so` built against CRuby's ABI. Install the ruby-platform variant
  (`bundle config set force_ruby_platform true`) and zeo compiles it.
* **Autotools and `mini_portile` builds of a vendored C library are out of
  scope.** System-library mode through `have_library` works.
* **Loading an extension arms the GVL** and turns off the lock-free
  container path, process-wide. That is a real cost and it is not optional:
  an extension may start a thread, and its C holds Ruby objects no other
  thread's view accounts for.
* **The shared object has to be there at run time.** An AOT binary dlopens
  it from the cache; it is not linked in. The gem report names the path.

### The C API surface

`crates/zeo-rt/src/cext/api.rs` is the census: every symbol an extension can link
against, read off clang's AST of every public header. 33 of them are
REFUSALS, not gaps, and each raises with its reason:

| What | Why |
|---|---|
| 21 `ruby_*` entries | they boot, configure or shut down an interpreter, and an extension loaded INTO a running one cannot |
| 5 `rb_big_*` entries | they expose MRI's Bignum digit array; `rb_integer_pack`/`rb_integer_unpack` are the supported way and both work |
| `rb_hash_tbl`, `rb_hash_bulk_insert_into_st_table` | they hand out a Ruby Hash's internal `st_table` |
| `rb_load_file`, `rb_load_file_str` | they answer a `NODE*`, MRI's parse tree |
| `rb_add_event_hook`, `rb_remove_event_hook` | the C-level TracePoint, whose event set does not line up |
| `rb_marshal_define_compat` | Marshal's internal compatibility table |

### Divergences worth knowing before you debug one

* **`RSTRING_PTR` pins.** A zeo String's bytes are a `Vec` behind a lock with
  no stable address, so the pointer is into a copy. The copy lives as long as
  the STRING does -- an extension may keep the pointer across C calls, which
  is what `msgpack`'s `feed_reference` needs and what MRI allows. What
  differs: a write through the pointer lands on the Ruby string at the next
  scope pop, so an extension that writes and then reads the string *through
  Ruby in the same C call* sees the old bytes. A Ruby-side mutation refreshes
  the copy at the next `RSTRING_PTR`, and a length change moves the address --
  MRI's moves on a resize too.
* **A Class handle is immortal.** Every `Init_` stores `cFoo` in a C static
  that outlives the call, and MRI can do that because a class is never
  collected. zeo keeps the handle for the process, which is one box per class
  an extension touches. Anything ELSE stored past its scope still needs
  `rb_gc_register_address`, exactly as on MRI.
* **`rb_thread_call_without_gvl`'s `ubf` is never called.** Interrupting
  opaque C means knowing what it is blocked on. A thread inside a C call is
  not killable until the call returns.
* **`rb_frame_this_func` and `rb_frame_callee` answer the same thing.** MRI
  separates the defined name from the called one, and an alias is what
  separates them; zeo's frame carries one label.
* **`RREGEXP_PTR` and `RMATCH_REGS` are refused.** Both reach into onig
  structures zeo's own engine owns, and handing out a pointer zeo may
  recompile behind would be worse than saying no. `Regexp` and `MatchData`
  answer the same questions through their methods.

### `dlopen` resolves eagerly

`RTLD_NOW`, where MRI uses `RTLD_LAZY`. MRI has no completeness promise to
keep; zeo does, and lazily a symbol it does not export binds to nothing and
faults at the call -- with no symbol name, no backtrace, and no way to tell a
zeo gap from a bug in the gem. Eagerly the same gap is a `LoadError` naming
the symbol.

## FFI — the real `ffi` gem, AOT-compiled

Zeo implements the **real `ffi` gem API**, not a custom DSL, so a program
using it runs identically under CRuby+ffi and Zeo (the north star). `require
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
C default argument promotions. The gem's Ruby half lives in `ext/ffi/`
(`Error`/`NullPointerError`, `Platform`, `DataConverter`); every NULL
read/write through a `Pointer` raises `FFI::NullPointerError` rather than
crashing.

**fiddle rides this.** `require "fiddle"` loads `crates/zeo-rt/ext/fiddle/` — fiddle
1.1.8's own `lib/fiddle/ffi_backend.rb` (its JRuby/TruffleRuby path)
vendored over the tier above, plus its `closure`/`function`/`version` files
verbatim. `Fiddle.dlopen`, `Fiddle::Function`, `Fiddle::Pointer`,
`Closure::BlockCaller` callbacks (qsort works), and `TYPE_VARIADIC` calls
are all oracle-matched against the C extension (`tests/fiddle.rb`); the
`Importer` DSL (`fiddle/import`/`fiddle/struct`) is not included — see
`docs/COMPATIBILITY.md` `### fiddle` for that and the other divergences.

## Out of scope (VM internals / tooling)

`rubyvm`, `continuation`, `ripper`, `win32`, `-test-`. `require`ing one raises
the normal `cannot load such file` — except where Zeo has a decision to state,
in which case the message carries it (`zeo_abi::declined_feature_reason`).
`ripper` is the case today: it points at `require "prism"`, the same parser
Zeo's own front end runs, which answers with a syntax tree rather than
parse.y's reduction stream. See "Declined" in
[`docs/COMPATIBILITY.md`](COMPATIBILITY.md).

## Adding an extension

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
