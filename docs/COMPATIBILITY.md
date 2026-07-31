# Library compatibility

## Encoding divergences (zeo-enc)

The encoding engine carries 27 encodings. The single-byte tables
(Windows-125x, ISO-8859-2/-15, KOI8-R) are generated from the ruby 4.0.6
oracle itself, so their mappings -- including which vendor-page bytes have
NO Unicode mapping -- are exact. Known divergences:

- **Shift_JIS mappings are CP932's.** Both the `Shift_JIS` and
  `Windows-31J` rows transcode through encoding_rs's WHATWG `shift_jis`
  table, which matches Windows-31J/CP932. CRuby's strict `Shift_JIS`
  differs in the NEC/IBM extension rows; a program that round-trips those
  extension characters through the `Shift_JIS` row gets CP932 answers.
  Structural validity (what counts as a character) is CRuby-faithful for
  both rows.
- **Big5 pairs that WHATWG maps to two-scalar sequences** (a handful of
  HKSCS combining forms) are treated as unmapped (undefined conversion)
  rather than decoded.
- **ISO-2022-JP undefined-conversion messages are simpler than CRuby's.**
  The dummy ISO-2022-JP row transcodes for real (the stateful escape codec
  in `enc/iso2022jp.rs`), and the representable repertoire matches -- but a
  character it refuses reports `U+XXXX from UTF-8 to ISO-2022-JP` where
  CRuby narrates its internal pivot chain (`"\x8F\xAB\xB1" to
  stateless-ISO-2022-JP in conversion from UTF-8 to EUC-JP to ...`).
  Halfwidth katakana are refused (as CRuby refuses them), and JIS X 0212
  characters CRuby reaches through its EUC-JP pivot are refused too (the
  EUC-JP row's encoder is JIS X 0208-only, per the CP932 note above).
- **The dummy UTF-16/UTF-32 rows always write a big-endian BOM** when
  encoding TO them, and reading FROM them requires one (no BOM is an
  invalid sequence) -- both CRuby-observed; the difference is only that
  error messages name the BE row (`UTF-16BE`) where CRuby says `UTF-16`.
- **String literals with raw high `\xNN` escapes** are not yet
  byte-faithful through the compiler's lowering (see
  [`docs/ROADMAP.md`](ROADMAP.md)); runtime-constructed bytes (`chr`, IO reads,
  `force_encoding`) are exact.

How zeo satisfies a `require`, and where its answer is **not** the upstream
gem or C extension. This is prose, not a percentage: each entry carries a
*reason*, because "zeo's `json` is not the `json` gem" is a fact that can
only be stated, never inferred from a score.

Every artifact-producing compile also writes a machine-readable
[`zeo-gems.json`](#the-per-compile-record) recording the same facts for the
libraries a given program actually used. This document is the human-facing
catalogue; that file is the per-program ledger.

## Satisfied, but divergent (a substitution)

zeo provides its own implementation under a name a gem or C extension also
uses. The surface is close, but the backing differs — so an edge case can
diverge, and `zeo-gems.json` marks these `diverges: true`. A compile warns
once per such library (slug `zeo-builtin-substitute`; silence with
`--nowarn=zeo-builtin-substitute`).

| `require` | zeo provides | why it diverges |
|---|---|---|
| `json` | `serde_json`-backed built-in | not the `json` gem; parser/generator options and error subclasses differ |
| `psych` / `yaml` | `yaml-rust2`-backed built-in | not libyaml; tag/anchor and error-position behaviour differ |
| `zlib` | `flate2`-backed built-in | not the `zlib` C extension; four entry points it doesn't expose are declined — see below |
| `digest` | RustCrypto-backed built-in | not the OpenSSL `digest` C extension |
| `openssl` | vendored OpenSSL 3 via rust-openssl | the same EVP primitives CRuby binds; PKey generation, X509 issuance and `SSLServer` are declined — see below |
| `strscan` | zeo `StringScanner` | a reimplementation, not the C extension |
| `stringio` | zeo `StringIO` | a reimplementation |
| `date` | zeo `Date`/`DateTime` | a reimplementation |
| `socket` | zeo `Socket` | a partial reimplementation |
| `base64` | zeo `Base64` | a reimplementation |
| `cgi` | zeo CGI escaping | escape/unescape only |
| `nkf` | zeo `NKF` over its own encoding engine | the conversion option subset only; `guess` is a reimplemented heuristic — see below |
| `bigdecimal` | zeo `BigDecimal` core + the gem's real Ruby half | not the C extension; the native slice is reimplemented — see below |
| `objspace` | always-on `ObjectSpace` rows | see below |
| `io/console` | always-on `IO` rows over `termios(3)` | see below |

### `objspace`

CRuby's `ext/objspace` adds its introspection methods to `ObjectSpace` when
required; zeo's are always present, so the `require` is ceremony (the shape
`io/wait` and `io/console` already have). What answers, and how:

- `memsize_of` computes from zeo's own value representation. CRuby documents
  the figure as implementation-defined and it is — only the shape is portable
  (0 for an immediate, growing with the payload).
- `reachable_objects_from` matches CRuby on everything a Ruby program can see
  (class first, then direct references, immediates dropped), but has no
  counterpart for the internal tier CRuby lists for a Class or a Proc —
  `T_ICLASS`, `T_IMEMO`, method entries — so those answer with their class
  alone.
- `count_symbols` reports the interner total as `immortal_symbol`; zeo never
  frees a symbol, so CRuby's mortal/dynamic/static split has no meaning here.
- `count_nodes`/`count_tdata_objects`/`count_imemo_objects` are empty because
  zero such objects exist, not because they couldn't be counted.
- The `allocation_*` getters answer nil — CRuby's own answer for an object
  allocated outside a trace, which under zeo is every object.
- `memsize_of_all`, `reachable_objects_from_root`, the
  `trace_object_allocations*` family, `dump`/`dump_all`/`dump_shapes`, and
  `internal_class_of`/`internal_super_of` raise `NotImplementedError` naming
  what they'd need (heap enumeration, a root set, an allocation hook, an
  object header, internal classes).

### `zlib`

The whole class surface is present and real — `ZStream`/`Deflate`/`Inflate`,
`GzipFile`/`GzipWriter`/`GzipReader`, the 38 constants, and the thirteen
exception classes (in `gems/zlib/lib/zlib.rb`, the gem's Ruby half). The
compression itself is flate2's pure-Rust backend (miniz_oxide), and the gzip
container is written and parsed by zeo, since that backend has no gzip mode.
What that costs:

- **Four entry points are declined**, each raising `NotImplementedError` that
  names the missing capability rather than no-op'ing: `Deflate#set_dictionary`,
  `Inflate#set_dictionary`/`#add_dictionary` (`deflateSetDictionary`/
  `inflateSetDictionary`), `Inflate#sync` (`inflateSync`), and `Deflate#params`
  (`deflateParams`). All four are `#[cfg]`'d to flate2's C-zlib and zlib-rs
  backends. A silent no-op was the wrong answer for the dictionary pair
  especially: it would produce a stream that decodes to the *wrong bytes*
  rather than one that fails. `params` records the level and strategy it was
  given, so a caller reading them back sees what it set; what cannot happen is
  the change taking effect mid-stream. CRuby's own `params` raises
  `Zlib::StreamError` in both natural call shapes and segfaults in a third
  (ruby 4.0.5, `rb_deflate_params`), so nothing depends on it working.
- **`window_bits` chooses the container, not the window size.** The offset —
  negative for raw deflate, +16 for gzip, +32 for auto-detect — is honoured
  exactly, including the `Zlib::Inflate.new(32 + Zlib::MAX_WBITS)` form
  `Net::HTTP` decodes response bodies with. The magnitude is not: the backend's
  window is fixed at 32KB, so `Deflate.new(9, 9)` writes a stream a decoder
  restricted to a 512-byte window could not read. Round-tripping is unaffected.
- **`mem_level` and `strategy` are accepted and ignored** — both size or steer
  zlib's internal tables, which the pure-Rust backend fixes. `avail_out=` is
  likewise recorded and reported back but inert, since zeo grows its own output
  buffer on demand.
- **`ZStream#data_type`** answers `TEXT`/`BINARY` from whether the block is all
  printable. zlib decides it from the literal histogram it builds while
  compressing, which the backend doesn't expose; the two agree on ordinary text
  and ordinary binary and can differ on a mixture.
- **Flush timing differs by a header.** zlib pushes the two-byte stream header
  out as soon as the first `deflate`/`<<` runs; miniz_oxide holds it until it
  has compressed data to go with it. So an intermediate `deflate(s)` can answer
  `""` where CRuby answers `"\x78\x9c"`. The concatenation of every chunk is
  the same valid stream either way — only the instalment boundaries move.
- **A corrupt deflate body** reports `Zlib::DataError: invalid or incomplete
  deflate data` where CRuby names the specific fault ("invalid distance too far
  back", "invalid code lengths set", …) — miniz_oxide carries no message.
  Corruption of the gzip *container* is exact: `CRCError`, `LengthError`,
  `NoFooter` and their CRuby wordings.
- **`Zlib::ZLIB_VERSION`** names the zlib API level implemented, not a linked
  libz — there isn't one. `Zlib::VERSION` is the bundled gem's, as CRuby's is.

One quirk is reproduced deliberately rather than fixed. CRuby verifies a gzip
member's CRC and length only once the buffer it filled has been fully handed
out, so `GzipReader#read` (which answers everything at once) never reports a
bad checksum while `#gets`/`#readlines`/`#readpartial` do. zeo follows the same
rule, so a program that reads a corrupt member with `read` gets the same bytes
under both.

`Zlib.gzip` used to stamp a fixed mtime of 0 for deterministic output. It now
stamps the current time, as CRuby does — a divergence removed, not added. A
caller who wants a reproducible member can set `GzipWriter#mtime=`.

### `io/console`

Its methods are unconditional rows on the `IO` table, so they answer before the
`require` as `io/wait`'s do. The terminal modes (`raw`/`raw!`/`cooked`/
`cooked!`/`noecho`/`echo=`/`echo?`/`getch`/`getpass`/`console_mode`), the
flushes, `winsize`/`winsize=`, `ttyname`, and the cursor/erase escapes are real
`termios(3)`/`ioctl` calls and match CRuby, `Errno::ENOTTY` messages included.
Two divergences:

- `pressed?` and `check_winsize_changed` raise `NotImplementedError`. That is
  CRuby's own behaviour on Unix, message included — they are Windows-only
  there too.
- `IO#cursor` asks the terminal for its position and parses the reply. A
  stream that answers nothing reads as `[0, 0]` rather than hanging.

Before this landed, `IO#winsize` answered `[0, 0]` on a non-terminal where
CRuby raises `Errno::ENOTTY`; it now raises. The corpus expectation that
recorded `[0, 0]` was spinel's, not ruby's, and has been re-oracled.

### `nkf`

`NKF.nkf`/`NKF.guess` are rebuilt over zeo's own encoding engine, not the
nkf C library, implementing what nkf fundamentally does -- decode under a
detected/declared Japanese encoding, apply text passes, re-encode. What is
faithful (oracle-verified byte-for-byte): the `-j/-e/-s/-w[8|16|32][B|L][0]`
outputs and their `-J/-E/-S/-W` input twins, `--ic=`/`--oc=` (unknown names
silently ignored, as nkf ignores them), MIME encoded-word decoding on by
default with adjacent-word joining (`-m`, off via `-m0`), halfwidth->
fullwidth katakana folding with voiced-mark combination on by default
(`-x` preserves halfwidth, including real `ESC ( I` runs for `-j -x`),
`-Z`/`-Z1`/`-Z2` (`～` exempt, as nkf leaves it), `-L[uwm]`, and the error
shapes (`no output encoding given`, the `TypeError`s). Divergences:

- **`guess` is a heuristic reimplementation.** The clear-cut cases (an
  ISO-2022 escape, pure ASCII, a BOM, text valid in exactly one of
  UTF-8/EUC-JP/Shift_JIS) answer as nkf does; ambiguous junk bytes may
  answer differently (nkf scores partial matches; this detector does not).
- **The rest of nkf's grammar is not pretended at.** MIME *encoding*
  (`-M`), fold (`-f`), `-h` hiragana/katakana swaps, `-I`, `-t`, and the
  long-option tail parse as no-ops, exactly like flags nkf itself does not
  know.
- **Broken input bytes are dropped** during decoding; real nkf's handling
  of malformed sequences is stream-state dependent and not promised.

### `bigdecimal`

bigdecimal 4.x splits itself between C and Ruby, and zeo keeps that split:
the native half (`crates/zeo-rt/src/ext/bigdecimal/`) reimplements exactly
the C slice -- the value type over a BigUint coefficient, exact
add/sub/mult, division to the documented rule (`max(a.precision,
b.precision) + double_fig`, floored at `2*double_fig`, rounded under the
current mode with a true sticky tail), the rounding engine, the
mode/limit/save_* state, conversions, and `Kernel#BigDecimal` -- while
`**`/`power`, `sqrt` (Newton), `BigMath`, and `util`'s `to_d` family are
the gem's OWN Ruby code, vendored in `gems/bigdecimal/` and compiled like
any user code. Both goldens (`tests/bigdecimal*.rb`) compare live against
the oracle, engineering-notation `to_s` and division digits included.
Known divergences of the native slice:

- **`Kernel#BigDecimal` answers without the require** (the `time`-shaped
  ceremony divergence: no per-method activation to hang the gate on).
- **ISO-2022-JP-style pivot messages don't apply here, but EUC-JP limits
  do**: values reachable only through JIS X 0212 (see the encoding notes
  above) behave per the encoding engine, not per libc nkf.
- The vendored Ruby half carries two marked deviations: the JRuby loader
  branch is reduced to `require "bigdecimal.so"`, and
  `private_class_method def` is unwrapped to a plain def (a compiler gap,
  `tests/gaps/issue_private_class_method_def.rb`) -- visibility-only.

### `fiddle`

fiddle 1.x ships its own pure-Ruby FFI backend (`lib/fiddle/ffi_backend.rb`,
the JRuby/TruffleRuby path), and that is the fiddle zeo runs: the backend is
vendored in `gems/fiddle/` over zeo's ffi runtime tier (`FFI::Type`,
`FFI::DynamicLibrary` over `dlopen(3)`, `FFI::Function` /
`FFI::VariadicInvoker` over libffi, `FFI.errno`) instead of the fiddle C
extension. The vendored backend carries `zeo:`-tagged deviations of two
kinds: mechanical ones (the `attach_function`-built `LibC` bindings rebuilt
over `DynamicLibrary` + `Function`; the `Types.const_get` symbol resolution
as an explicit table; the LP64 collapse of the type-size `if` chains) and
CRuby-alignment ones, where the backend's own behavior differs from the C
extension the oracle runs (error messages like `unknown symbol "x"` /
`unknown type 99` / `wrong argument type Integer (expected Array)`, dlopen
failures carrying the raw `dlerror(3)` text, `NULL.to_s` raising
`ArgumentError "NULL pointer given"`, `Pointer#==` answering `false` for a
non-Pointer, and `Fiddle.malloc` returning the raw address Integer of a real
libc `malloc`). The golden (`tests/fiddle.rb`) compares all of this live
against the C-extension oracle. Known divergences:

- **`fiddle/import` and `fiddle/struct` are not included** (nor
  `types`/`pack`/`value`/`cparser`, which exist to serve them): the
  `Importer` DSL builds its methods with `module_eval` on computed strings,
  which AOT compilation can't express. `require`ing them is a `LoadError`.
- **`Handle#sym_defined?` answers true/false** (the backend's behavior);
  the C extension leaks the address-or-nil, which is only ever used as a
  truthy.
- **`Fiddle.dlwrap` answers a `Fiddle::Pointer`** (for a String, over a
  malloc'd copy of its bytes -- the backend's behavior); the C extension
  returns the object's VALUE address, which has no zeo equivalent.
- **`Pointer#inspect` carries no object id** and prints the backend's
  format; the C extension's includes the Ruby object address
  (nondeterministic in either case).
- **NULL dereference raises instead of crashing**: `Fiddle::NULL.ptr` and
  writing through address 0 SEGFAULT the C extension; here they raise
  (`DLError` / `FFI::NullPointerError`), as the backend does.
- **`TYPE_CONST_STRING` returns answer a `Pointer`, not a `String`** (the
  backend maps `CONST_STRING` to `POINTER`); as a variadic *argument* type
  it converts correctly.
- `Fiddle::Closure#free` is bookkeeping only -- the libffi closure lives as
  long as the object (the gem frees it eagerly; calling through a freed
  closure is undefined behavior there, an error here).

### `coverage`

CRuby's coverage extension instruments iseqs as the VM compiles them; zeo
has no VM, so requiring `coverage` makes the COMPILER emit the
instrumentation instead -- a hit counter beside every statement's line
stamp, a load mark where each spliced file's top level begins, and a
per-file coverable-line table. A program that doesn't require `coverage`
carries none of it. The lifecycle
(`start`/`setup`/`resume`/`suspend`/`result`/`peek_result`/`running?`/
`state`), the inclusion rule (a file is reported iff its top level began
while measurement was set up; the entry script never qualifies), the
nil/0/count line shapes, and every error message are oracle-matched live
(`tests/coverage.rb`). Known divergences:

- **Lines only**: `supported?(:lines)` is true; `:branches`, `:methods`,
  `:oneshot_lines` and `:eval` answer FALSE (CRuby supports them), and a
  `start`/`setup` requesting one raises `RuntimeError`. Tools that check
  `supported?` first (simplecov does) degrade gracefully.
- **`def` lines report the definition count statically**: a definition
  executes once, at its file's load, so a covered file's `def` lines report
  1. A definition statement re-executed at runtime (a `def` inside a method
  body called n times) still reports 1 where CRuby reports n.
- **Statically-decided code isn't coverable**: a top-level `if` branch the
  compiler splices away (`if defined?(Ractor)` guards) reads nil where
  CRuby reads 0 -- the code was never compiled, the same reason the branch
  can't raise. Class-body DIRECTIVE lines the compiler consumes
  (`attr_accessor`, `private`, `include`) read nil for the same reason.
- **Result keys are the compile-time span paths** (build-machine paths for
  bundled-gem files) -- the `$LOAD_PATH`/`$LOADED_FEATURES` caveat again.
- `Coverage.line_stub` is not implemented: it parses a source file at
  runtime, and an AOT binary ships no parser.

### `TracePoint`

CRuby's TracePoint hooks the VM's trace instructions; zeo has no VM, so
`TracePoint` rides the instrumentation the runtime already carries for
backtraces: the per-statement line stamp fires `:line`, the frame
push/pop pair fires `:call`/`:return` (and `:class`/`:end`, classified
by the frame's label), and the raise channel fires `:raise`. When no
tracepoint is enabled each hook is one relaxed atomic load. The
lifecycle (`enable`/`disable` answering the previous state, their
block-scoped forms, `enabled?`, `TracePoint.trace`), `event`/`path`/
`lineno`/`method_id`/`callee_id`/`defined_class` (singleton classes
included: `#<Class:Foo>`)/`raised_exception`, reverse-enable-order
dispatch across multiple tracepoints, reentrancy suppression while a
handler runs, `:return` firing when an exception unwinds a method, and
the inspect/error shapes (`"access from outside"`, `"not supported by
this event"`, `"unknown event: x"`) are oracle-matched live
(`tests/tracepoint.rb`). Known divergences:

- **The six reachable events only**: `:b_call`/`:b_return` (blocks),
  `:c_call`/`:c_return` (builtins run as native code, not method
  frames), `:rescue`, `:thread_begin`/`:thread_end`, `:fiber_switch`,
  and `:script_compiled` never fire, so `TracePoint.new` naming one
  raises `RuntimeError: event :x is not supported by zeo` where CRuby
  accepts it -- loud, not a handler that silently never runs.
- **`#self`, `#binding`, `#return_value`, `#parameters`,
  `#eval_script`, `#instruction_sequence` are not implemented**: the
  lightweight frame deliberately carries no receiver or bindings.
- **An explicit early `return` reports the `end` line** for `:return`
  where CRuby reports the `return` statement's line (the frame pop
  cannot tell the exit paths apart; an exception unwind reports the
  `end` line too, where CRuby reports the raise line).
- **`def` lines fire no `:line`**: definitions are compile-time (the
  coverage `def`-line caveat again). Class-body directive lines the
  compiler consumes (`attr_accessor`, `include`) are likewise silent.
- **Top-level lines of a required file report the entry file's path**:
  spliced top-level code runs under the `<main>` frame. Code inside
  methods and class bodies reports its own file.
- **`callee_id` equals `method_id`** for an aliased call (labels carry
  the defining name).
- **A handler that raises aborts the program** with the uncaught
  report after `at_exit`/finalizers -- there is no `Result` channel
  from inside a line stamp or a frame pop. Observably close to CRuby,
  where the propagated exception is not catchable by a `rescue` around
  the traced call either.
- `enable(target:)`/`enable(target_line:)` filtering and
  `TracePoint.allow_reentry` are not implemented.

### `openssl`

Backed by the official rust-openssl bindings over a **vendored OpenSSL
3.x**, so the digest, cipher, BN and TLS primitives are the same EVP
implementations CRuby's C extension binds — algorithm outputs, tags and
error messages match by construction rather than by reimplementation.
What differs is the surface around them.

**Shipped**: `Digest` (+ `MD4`/`MD5`/`RIPEMD160`/`SHA1`/`SHA224`/
`SHA256`/`SHA384`/`SHA512`), `HMAC`, `KDF.pbkdf2_hmac`/`hkdf`/`scrypt`
(and the `PKCS5` shim), `BN`, `Cipher`, `Random`, the secure compares,
the version constants, and CLIENT-side `SSL::SSLContext`/`SSLSocket`
with `X509::Store`/`Certificate`.

**Declined** (raise `NoMethodError`/`NameError` rather than pretending):
`PKey` key generation and signing, X509 certificate issuance, PKCS#7,
ASN1, `SSLServer` and TLS `accept`. Zeo compiles clients, not
certificate authorities.

Divergences:

- **`OpenSSL::Digest` sits under `Object`**, not under the `digest`
  framework's `Digest::Class`; zeo's digest classes are native tables
  with no shared Ruby superclass. `is_a?(Digest::Instance)` answers
  false, though every method that contract names is present.
- **Camellia, IDEA and SEED are absent from the vendored build**
  (openssl-src's defaults): their names appear in `Cipher.ciphers` —
  which reports CRuby's own NID table — but `Cipher.new` raises
  `CipherError`, the same way RC4 and Blowfish do on both runtimes
  (OpenSSL 3 moved those to the legacy provider).
- **`Cipher::AES`/`AES256`-style shorthand subclasses are absent.**
  Spell the algorithm out: `Cipher.new("aes-256-cbc")`.
- **The trust store is configured, not enumerated.** `X509::Store`'s
  mutators are accepted and do nothing; what decides trust is the
  context's `ca_file` or, failing that, the first existing system CA
  bundle (the probe list from upstream's own `openssl.rb`, since a
  vendored library's compiled-in cert path belongs to the build machine).
- **`read_nonblock`/`write_nonblock`/`connect_nonblock` block.** They
  drive a blocking descriptor and never answer `:wait_readable`, so a
  caller's own socket timeout does not interrupt them.
- **`Certificate#subject`/`#issuer` answer a String** (OpenSSL's
  one-line DN), where CRuby answers an `X509::Name` whose `to_s` is that
  string.
- Session resumption (`SSLSocket#session`), ALPN, client certificates
  and the verify/session callbacks are not implemented;
  `session_cache_mode` is carried but inert.

`post_connection_check` is NOT a divergence: it runs the same RFC 6125
identity check upstream's `verify_certificate_identity` does (SAN
dNSName/iPAddress deciding when present, CN only in their absence,
left-most-label wildcards), independent of the connection's verify mode.

### `Binding`

`Kernel#binding` is a core surface, not a substitution: `#receiver`,
`#source_location`, `#local_variables`, `#local_variable_get`/`_set`/
`_defined?`, `#eval`, `#dup`/`#clone` and `TOPLEVEL_BINDING` all behave as
CRuby's, sharing the compiled frame's own slots so writes flow both ways
(`docs/EVAL_VM.md` explains how codegen gives just those frames cell
storage). The bounds:

- **A Binding taken inside an INLINE-SPLICED iterator block** (`n.times { |i|
  ... }` and the other bodies zeo splices into the enclosing Rust scope
  rather than building a `Proc` for) carries that block's own parameters only
  when the enclosing scope is already a binding scope. Elsewhere the spliced
  parameter is a plain per-iteration `let` with no cell to share, so it is
  ABSENT from `local_variables` rather than wrongly bound. Real (escaping)
  blocks and lambdas carry theirs unconditionally.
- **`Binding#irb`**, `#implicit_parameter_get`/`_defined?`/`#implicit_parameters`
  are not implemented.
- **A `send` whose method name is COMPUTED** (`m = :eval; send(m, src)`) is
  invisible to static analysis: that binary may not link the eval VM at all,
  and the eval gets no scope. A LITERAL `send(:eval, src)` /
  `obj.send(:eval, src)` is fully supported — same locals, same `self` rule as
  CRuby's. `public_send(:eval, …)` is `NoMethodError` in CRuby because
  `Kernel#eval` is private; zeo does not enforce that visibility, so it
  evaluates instead (in a scope-less context, so a caller local is a
  `NameError`).
- **`method(:eval).call(src)`** does not see the caller's locals — a `Method`
  carries no binding.

`Proc#binding` IS implemented, and is not a substitution either: it answers a
Binding of the scope the block was written in (that scope's `self` and locals,
shared by reference), never the block's own locals. A proc with no Ruby scope
behind it — `Symbol#to_proc` and the other runtime-internal ones — raises
CRuby's `ArgumentError: Can't create Binding from C level Proc`, and so does any
proc in a program the compiler never saw ask for a `Proc#binding` (the capture
is pay-per-use; see `docs/EVAL_VM.md`).

## Satisfied faithfully (zeo-bundled gems)

Zeo ships its own copy under `gems/<name>/`, intended to match upstream
behaviour. Recorded `by: bundled-gem` with no divergence flag. These are
subsets where noted, not substitutions with a foreign backing.

| `require` | notes |
|---|---|
| `optparse` | `OptionParser` — the common surface |
| `monitor` | `Monitor` + `MonitorMixin` |

## The installed Ruby's own stdlib

A plain-Ruby stdlib library reached over a `-I` load root (the installed
oracle's `rubylibdir`) is zeo-compiled as-is and recorded `by: stdlib-root`.
No substitution is involved — it is the real stdlib source.

## Not available (native gems)

A gem whose real implementation is a C extension zeo has no built-in for
cannot be compiled. The `require` fails with a message that **names the gem**
rather than looking like an unsupported language feature, and points here and
at the FFI path (see `docs/EXTENSIONS.md`), zeo's intended escape hatch.
Examples that trigger the named error today: `sqlite3`, `nokogiri`, `pg`,
`mysql2`, `bcrypt`, `nio4r`, `grpc`, `msgpack`, and similar.

## Declined (a CRuby internal, not a missing binding)

Two stdlib extensions expose CRuby's own machinery rather than a library, so
there is nothing for zeo to bind — reproducing them means rebuilding the
machinery. `require` raises `LoadError`, which is a divergence from ruby, but a
settled one rather than a queued job.

The `LoadError` **names the decision** where zeo has one to state
(`loader.rs::declined_reason`), so a caller can tell a decline from a typo or
an unfinished feature.

- **`continuation`** (`Kernel#callcc`) captures and restores the machine
  stack. zeo compiles to native Rust and has no stack-copying runtime. An
  escape-only `callcc` — enough for an upward jump out of a nested call — is
  reachable, and deliberately not shipped: it would answer the common case
  and silently break re-entry, which is worse than a `LoadError` a caller can
  rescue. CRuby itself prints *"callcc is obsolete; use Fiber instead"* when
  the extension loads, and zeo ships `Fiber`. Tracked as a divergence in
  `tests/gaps/issue_continuation_missing.rb`, since ruby does load it.
- **`ripper`** exposes the reduction event stream of CRuby's `parse.y`. zeo's
  front end embeds prism, a different parser with a different event model, so
  a binding has nothing to bind to; matching ripper means re-implementing
  CRuby's grammar actions. **`require "prism"` is the answer instead** — the
  real gem's Ruby half over the same prism zeo itself parses with, which gives
  a syntax tree rather than a reduction stream. The decline is asserted by
  `ripper_is_declined_and_the_load_error_says_so`
  (`crates/zeo/tests/e2e/gems_require.rs`) rather than by a golden: a golden is
  diffed against the oracle, and ruby loads ripper, so the two can never agree.

## Refinements

`Module#refine` and `Kernel#using` work, with real Ruby's lexical rule: a
`using` covers everything written after it, to the end of the enclosing body
(the end of the file at the top level). A `def` written after the `using` sees
the refinement; one written above it does not. `send`, `public_send`,
`respond_to?` and `Object#method` all honour it, `Module#instance_methods`
does not, and a refined `Method#owner` reports a `Refinement` — see
`tests/refinements.rb`, which is oracle-blessed line by line.

Three narrowings:

* The target must be a **literal constant**. `refine Object.const_get(:String)`
  falls through to an ordinary call, which raises.
* `using` must name a **literal constant** module, for the same reason.
* `Module#refinements`, `Module#used_modules` and `Refinement#refined_class`
  are not implemented — the refinement objects exist, but nothing enumerates
  them.

A refined call site gives up its static dispatch: whether the refinement
applies depends on the receiver's runtime class, so the call routes through one
runtime entry point that tries the refined bodies and then falls back to an
ordinary send. That cost is paid only where a `using` and a refined name
actually meet.

## Required only from a method body

A plain `require` that **only a method body** reaches is not compiled in.

```ruby
def render
  require "erb"          # not loaded -- raises LoadError when `render` runs
  ERB.new(@src).result
end
```

CRuby loads `erb` when `render` is first called. Zeo has no runtime loader, so
it must either load the file at program start or not at all, and loading it
early is worse than it looks: the file lands *ahead* of the requires its own
file makes at top level, and every lazy dependency is drawn into the binary.
`rubygems.rb` shows both faults at once — `Gem.use_gemdeps` says
`require "bundler"`, which ran bundler's `rubygems_ext` against a
`Gem::Specification` whose class body had not executed yet, and inflated
`require "rubygems"` from 779K generated lines to 2.69M.

So the call is left alone and becomes a runtime `Kernel#require`. It answers
`false` if any load-time position did require the library, and otherwise raises
CRuby's own `LoadError` at the require site — rescuable, and never silently
wrong output. Compile time discloses it too: a `deferred-require` entry in
`zeo-gems.json` and one warning per library.

To compile the library in, require it from any position the file's **load**
executes — the top level, a conditional, a `begin`, a class body:

```ruby
require "erb"            # now compiled in; the one inside `render` answers false
```

A `require_relative` is exempt from all of this. It names a file of the same
program rather than a library boundary, so it is always loaded. One inside a
method body lands at the **end** of the file that writes it: the method cannot
run before its own file has finished loading, and the target routinely reopens
a class that file is still building. irb's `context.rb` requires
`ext/eval_history.rb` from inside `eval_history=`, and that file pushes onto a
`NOPRINTING_IVARS` assigned further down `context.rb`.

## Where a nested `require` lands

A require the file's load reaches but that is not a top-level statement — one
under a conditional, in a `begin`, in a class body, in a block — is spliced at
the position of the **statement that holds it**:

```ruby
require_relative "../minitest"          # first
require_relative "spec"                 # second
require_relative "hell" if ENV["MT_HELL"]   # third, here -- not at the top
```

The guard itself is not evaluated at compile time (only the platform-detection
idioms `RUBY_ENGINE == "jruby"` and friends are), so a require in a branch that
never runs is still compiled in and still executes. That is usually invisible —
every extension links statically — but not always: `minitest/hell.rb` calls
`parallelize_me!` and warns about a missing optional gem, neither of which
CRuby does when `MT_HELL` is unset.

A file loaded this way runs its top level at program START, not on the call, so
a lazily-required file that only means to *warn and bail* on a missing optional
dependency does both at startup instead. `irb/ext/tracer.rb` is one; see
`gems/UPSTREAM.md` for how it is vendored.

## A class written in a `class << self` body

```ruby
module Color
  class << self
    class Visitor; end        # belongs to Color's SINGLETON class
    def paint = Visitor.new   # ...which is what makes this bare name resolve
  end
end
```

Real Ruby puts `Visitor` on `Color.singleton_class`: the singleton methods
beside it see it by bare name, `Color.constants` is empty, and `Color::Visitor`
raises `NameError`. Zeo hands the definition to the enclosing module instead.
The bare-name lookup -- the only reason such a class is ever written there --
works; the divergence is that `Color::Visitor` also answers, and `Visitor`
shows up in `Color.constants`.

`tests/singleton_body_class_and_self_path.rb` pins the behaviour, with the
oracle's answers recorded beside zeo's.

## A top-level `return` inside a required file

`return` at the top level ends the program. In a file the main script
`require`s, real Ruby ends only THAT file's load and carries on in the
requirer; zeo splices required files into their requirer, so the `return`
reaches the top level of the whole program and ends it. Exit status stays 0 and
`at_exit` handlers still run, both matching a top-level `return` in the main
script.

## `extend` on a class, at runtime

`Klass.extend M` written as a runtime call — as opposed to `extend M` in the
class body — installs each of `M`'s methods as a class method of `Klass`. The
body runs with `self` bound to the class, so an implicit-self class-method call
and an `@ivar` write both land where Ruby says: `@x` is the class's own
class-level slot, the same one a `def self.x` reads. The singleton gem depends
on exactly this (`klass.extend SingletonClassMethods`, then
`klass.instance_eval { set_mutex(Thread::Mutex.new) }`).

What does **not** follow is the ancestry: zeo installs the methods rather than
splicing `M` into the class's singleton chain, so `Klass.singleton_class.
include?(M)` answers false where real Ruby answers true. Dispatch,
`respond_to?`, and the method's own behaviour are unaffected.

## Compiling against an installed gem store

`zeo app.rb --gem-path "$(gem env gemdir)" --lockfile Gemfile.lock` resolves
the gems your `Gemfile.lock` locked out of the installed RubyGems store. zeo
consumes Bundler's resolution verbatim — it never resolves, fetches, or builds
extensions — and applies TruffleRuby's `force_ruby_platform`: it uses the
`ruby`-platform (source) gemspec, never a precompiled `.bundle`. Both flags are
required together and are explicit opt-in (a compile that silently depended on
`$GEM_HOME` would not be reproducible).

Each locked gem lands in one of three buckets, all recorded in
`zeo-gems.json`:

| bucket | what happens |
|---|---|
| pure Ruby | compiled — added as a require-path root, the majority case |
| name zeo provides natively (`json`, `psych`, …) | satisfied by zeo's built-in; the store copy is ignored and the divergence recorded |
| native, unknown to zeo | **excluded** — recorded with a reason, and a `require` of it fails naming the layout (a locally-built extension, or a precompiled-platform-only install) and pointing at the FFI path |

An excluded gem that the program never `require`s costs nothing but a
disclosure line — an AOT compiler only compiles what a require actually
reaches. `GIT`/`PATH`-source gems (a checkout or a local path in the lockfile)
are not drawn from the store.

To measure the out-of-the-box number, run `cargo xtask gem-compat` — with no
argument it classifies **every gem installed** in the store (`Gem.dir` by
default), or pass a `<Gemfile.lock>` to measure just its locked subset, and
`--gem-path <dir>` for a different store. It prints a per-gem table and a
headline like *"163/195 store gems usable (84%) — 151 compiled, 12 built-in, 32
native unsupported"*, and writes `conformance/gem-compat.{tsv,md}` with the
native gems grouped by detected layout (the FFI work-list).

## The per-compile record

`zeo app.rb -o app` writes `zeo-gems.json` next to the artifact, one
object per library the program required:

```json
{
  "json":     {"by": "bundled-gem", "path": "gems/json/lib/json.rb",
               "diverges": true, "note": "serde_json-backed; not the json gem"},
  "optparse": {"by": "bundled-gem", "path": "gems/optparse/lib/optparse.rb"},
  "base64":   {"by": "builtin-ext", "feature": "base64",
               "diverges": true, "note": "a zeo reimplementation of Base64"},
  "erb":      {"by": null, "excluded": "deferred-require",
               "reason": "required only from a method body, ..."}
}
```

It is written **by default** — the substitution is silent by nature, so the
record has to already be on disk at the moment a user discovers they need it.
`--no-report` opts out for callers that already know substitutions happen (the
conformance, example, and test harnesses).
