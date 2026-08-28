# Library compatibility

## Encoding divergences (zeo-enc)

The registry carries all 103 of ruby 4.0.6's encodings, in its order, so
`Encoding.list`, `.name_list`, `.aliases`, `.find`, every `Encoding::*`
constant and every `#name`/`#names`/`#dummy?`/`#ascii_compatible?` answer
match exactly -- see [`tests/encoding_registry.rb`](../tests/encoding_registry.rb).
The 52 single-byte mapping tables (the ISO-8859, windows-125x, IBM/CP,
KOI8, mac* and Thai families) are derived from the oracle itself, so their
mappings -- including which vendor-page bytes have NO Unicode mapping --
are exact. Known divergences:

- **Shift_JIS mappings are CP932's.** Both the `Shift_JIS` and
  `Windows-31J` rows transcode through encoding_rs's WHATWG `shift_jis`
  table, which matches Windows-31J/CP932. CRuby's strict `Shift_JIS`
  differs in the NEC/IBM extension rows; a program that round-trips those
  extension characters through the `Shift_JIS` row gets CP932 answers.
  Structural validity (what counts as a character) is CRuby-faithful for
  both rows.
- **Big5 pairs that WHATWG maps to two-scalar sequences** (a handful of
  HKSCS combining forms) are treated as unmapped (undefined conversion)
  rather than decoded. DECODE only: Big5's ENCODE repertoire matches CRuby
  exactly, scalar for scalar and byte for byte, pinned by
  `tests/big5_and_euc_jp_encode_exactly.rb`.
- **ISO-2022-JP undefined-conversion messages are simpler than CRuby's.**
  The dummy ISO-2022-JP row transcodes for real (the stateful escape codec
  in `enc/iso2022jp.rs`), and the representable repertoire matches -- but a
  character it refuses reports `U+XXXX from UTF-8 to ISO-2022-JP` where
  CRuby narrates its internal pivot chain (`"\x8F\xAB\xB1" to
  stateless-ISO-2022-JP in conversion from UTF-8 to EUC-JP to ...`).
  Halfwidth katakana are refused (as CRuby refuses them), and so are the
  JIS X 0212 characters CRuby reaches through its EUC-JP pivot -- CRuby
  refuses those too, it just names the pivot while doing so. (The
  parenthetical here until 2026-08-26 said the EUC-JP encoder is JIS X
  0208-only. It no longer is: EUC-JP reaches the SS3 plane, and
  ISO-2022-JP's repertoire did not change with it, because ISO-2022-JP
  has no room for those characters either way.)
- **The dummy UTF-16/UTF-32 rows always write a big-endian BOM** when
  encoding TO them, and reading FROM them requires one (no BOM is an
  invalid sequence) -- both CRuby-observed; the difference is only that
  error messages name the BE row (`UTF-16BE`) where CRuby says `UTF-16`.
- **`#putback` answers an empty String**, for the same reason a converter
  never needs it: Zeo's decoders consume an offending sequence whole, so
  `#primitive_errinfo`'s fifth element is always empty.

How Zeo satisfies a `require`, and where its answer is **not** the upstream
gem or C extension. This is prose, not a percentage: each entry carries a
*reason*, because "Zeo's `json` is not the `json` gem" is a fact that can
only be stated, never inferred from a score.

A compile run with `--report` also writes a machine-readable
[`zeo-gems.json`](#the-per-compile-record) recording the same facts for the
libraries a given program actually used. This document is the human-facing
catalogue; that file is the per-program ledger.

## Ractor, IO::Buffer, RubyVM, Ruby::Box

These subsystems carry their full census surface; the divergences below are
behavioural, each deliberate, each narrower than the CRuby behaviour it
replaces.

- **A refused `move:` send poisons nothing.** CRuby's move traversal guts
  objects as it walks, so `r.send([a, Thread.current], move: true)` destroys
  `a` before raising on the Thread. Zeo validates the whole graph first;
  a refusal leaves every object intact. Related: CRuby delivers a
  `Ractor::MovedObject` husk for the second occurrence of a duplicated
  reference in a moved graph (`[x, x]`); Zeo preserves the duplicate as one
  moved object. Cycles reconstruct on both engines.
- **`IO` objects do not move.** CRuby migrates an IO across a `move:` send
  (poisoning `$stdout` included); Zeo refuses with `can not move IO object.`
  A moved `Range` keeps its shell in Zeo (Range is an inline value here)
  where CRuby poisons it.
- **`IO::Buffer#resize` never invalidates a slice.** The backing grows in
  place and never shrinks, so a slice taken before a resize keeps answering
  (CRuby's slice also answers, over memory it happens not to have moved).
- **`RubyVM::AbstractSyntaxTree` node ids are zeo-numbered.** CRuby's AST
  ids are a third numbering, neither prism's nor zeo's, so reproducing this
  row means reproducing CRuby's own allocation order. Prism's real ids ARE
  read, for `node_id_for_backtrace_location`. Types, locations,
  children orderings, `#source` and `#script_lines` match the oracle (pinned
  by `tests/rubyvm_ast.rb`); a construct outside the mapped tier answers an
  honest `:UNKNOWN` leaf rather than raising. `SyntaxError` messages carry
  prism's wording, not parse.y's.
- **`RubyVM::InstructionSequence` refuses serialization.** `#to_a`,
  `#to_binary` and the disassembly family raise `NotImplementedError` naming
  the reason: Zeo compiles ahead of time and has no YARV bytecode.
  `compile`/`#eval` are real (a prism parse check, then a compile).
  `InstructionSequence.of` answers a real object naming the method and its
  source site (`<RubyVM::InstructionSequence:m@file:line>`); only the
  serialization rows on it refuse. The remaining refusals are recorded in
  `tests/gaps/rubyvm_iseq_serialization.rb`, whose header reads the rows
  apart (which are permanent absences, which are open work).
- **`RubyVM::YJIT.enable` answers `false`.** There is no JIT to switch on.
  Every stats/log reader answers its disabled shape.
- **`Ruby::Box` is real at run time.** `box = Ruby::Box.new` as a top-level
  statement allocates a box in the compiler, and a computed (run-time)
  `Box.new` allocates one too; `box.eval` isolates its top-level constants
  against the box (a dynamic `box.eval("X = 1")` lands in the box, not on
  `Object`). `Ruby::Box.current` answers the current box. `#require` and
  `#load` are real, but a computed or box-side target can only reach sources
  the binary embeds — `--embed-sources` is the opt-in (the decided
  divergence in `tests/a_box_cannot_require_a_spliced_feature.rb`). Without
  `RUBY_BOX=1`, `Box.new` raises the same disabled-mode `RuntimeError` ruby
  raises.

## Satisfied, but divergent (a substitution)

Zeo provides its own implementation under a name a gem or C extension also
uses. The surface is close, but the backing differs — so an edge case can
diverge, and `zeo-gems.json` marks these `diverges: true`. Ask for that
record with `--report`: it is the disclosure channel. A compile prints
nothing, because a diagnostic on every run is noise on the program's own
stderr that Ruby never produces.

| `require` | Zeo provides | why it diverges |
|---|---|---|
| `json` | a hand-written parser and generator | not the `json` gem's C extension. It targets json 2.21.2 and matches it row for row on the option and error matrix; both halves are ITERATIVE, so `max_nesting: false` costs heap rather than machine stack, which is a stronger guarantee than the gem's own generator gives. The serde_json substitution this row used to describe is long retired. |
| `psych` / `yaml` | `yaml-rust2` parsing, psych's semantics above it | not libyaml. Tags, anchors, merge keys, `!ruby/object:` revival and the `Psych::Nodes` tree all behave as psych's do; what still differs is `Psych::SyntaxError`'s problem TEXT and a node tree's `#yaml` (see tests/gaps/). |
| `zlib` | `flate2`-backed built-in | not the `zlib` C extension; four entry points it doesn't expose are declined — see below |
| `digest` | RustCrypto-backed built-in | not the OpenSSL `digest` C extension |
| `openssl` | vendored OpenSSL 3 via rust-openssl | the same EVP primitives CRuby binds; PKey generation, X509 issuance and `SSLServer` are declined — see below |
| `strscan` | Zeo `StringScanner` | a reimplementation, not the C extension |
| `stringio` | Zeo `StringIO` | a reimplementation |
| `date` | Zeo `Date`/`DateTime` | a reimplementation |
| `socket` | Zeo `Socket` | a partial reimplementation |
| `base64` | Zeo `Base64` | a reimplementation |
| `cgi` | Zeo CGI escaping | escape/unescape only |
| `nkf` | Zeo `NKF` over its own encoding engine | the conversion option subset only; `guess` is a reimplemented heuristic — see below |
| `bigdecimal` | Zeo `BigDecimal` core + the gem's real Ruby half | not the C extension; the native slice is reimplemented — see below |
| `objspace` | always-on `ObjectSpace` rows | see below |
| `io/console` | always-on `IO` rows over `termios(3)` | see below |

### `objspace`

CRuby's `ext/objspace` adds its introspection methods to `ObjectSpace` when
required; Zeo's are always present, so the `require` is ceremony (the shape
`io/wait` and `io/console` already have). What answers, and how:

- `memsize_of` computes from Zeo's own value representation. CRuby documents
  the figure as implementation-defined and it is — only the shape is portable
  (0 for an immediate, growing with the payload).
- `reachable_objects_from` matches CRuby on everything a Ruby program can see
  (class first, then direct references, immediates dropped), but has no
  counterpart for the internal tier CRuby lists for a Class or a Proc —
  `T_ICLASS`, `T_IMEMO`, method entries — so those answer with their class
  alone.
- `count_symbols` reports the interner total as `immortal_symbol`; Zeo never
  frees a symbol, so CRuby's mortal/dynamic/static split has no meaning here.
- `count_nodes`/`count_tdata_objects`/`count_imemo_objects` are empty because
  zero such objects exist, not because they couldn't be counted.
- The `allocation_*` getters answer nil — CRuby's own answer for an object
  allocated outside a trace, which under Zeo is every object.
- `each_object` and `memsize_of_all` walk the allocation registry, so they
  need `ZEO_GC=1` for the instance forms and refuse without it. `each_object`
  answers each argument form either COMPLETELY or not at all, because a
  partial enumeration presented as a whole one silently miscounts:
  `Class`/`Module` is complete and needs nothing armed (a class is a
  registered row, not an allocation); a class the registry records -- an
  Object and its subclasses, Array, Hash, Proc, Range -- is complete while
  recording; anything else, and the no-argument form, refuses, because a
  String, a Symbol and an Integer are never registered and "every object" is
  not a set Zeo can produce. The block form answers the count; there is no
  Enumerator over a walk that cannot be resumed.
- `reachable_objects_from_root`, the `trace_object_allocations*` family,
  `dump`/`dump_all`/`dump_shapes`, `_id2ref`, and
  `internal_class_of`/`internal_super_of` raise `NotImplementedError` naming
  what they'd need (a root set, per-allocation source positions, an object
  header, an id table, internal classes). See
  `tests/gaps/object_identity_and_allocation_tracing.rb`.

### `zlib`

The whole class surface is present and real — `ZStream`/`Deflate`/`Inflate`,
`GzipFile`/`GzipWriter`/`GzipReader`, the 38 constants, and the thirteen
exception classes (in `ext/zlib/lib/zlib.rb`, the gem's Ruby half). The
compression itself is flate2's pure-Rust backend (miniz_oxide), and the gzip
container is written and parsed by Zeo, since that backend has no gzip mode.
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
  likewise recorded and reported back but inert, since Zeo grows its own output
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
bad checksum while `#gets`/`#readlines`/`#readpartial` do. Zeo follows the same
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

`NKF.nkf`/`NKF.guess` are rebuilt over Zeo's own encoding engine, not the
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

bigdecimal 4.x splits itself between C and Ruby, and Zeo keeps that split:
the native half (`crates/zeo-rt/ext/bigdecimal/ext/bigdecimal/src/`) reimplements exactly
the C slice -- the value type over a BigUint coefficient, exact
add/sub/mult, division to the documented rule (`max(a.precision,
b.precision) + double_fig`, floored at `2*double_fig`, rounded under the
current mode with a true sticky tail), the rounding engine, the
mode/limit/save_* state, conversions, and `Kernel#BigDecimal` -- while
`**`/`power`, `sqrt` (Newton), `BigMath`, and `util`'s `to_d` family are
the gem's OWN Ruby code, vendored in `crates/zeo-rt/ext/bigdecimal/` and compiled like
any user code. Both goldens (`tests/bigdecimal*.rb`) compare live against
the oracle, engineering-notation `to_s` and division digits included.
Known divergences of the native slice:

- **`Kernel#BigDecimal` answers without the require** (the `time`-shaped
  ceremony divergence: no per-method activation to hang the gate on).
- **ISO-2022-JP-style pivot messages don't apply here, but EUC-JP limits
  do**: values reachable only through JIS X 0212 (see the encoding notes
  above) behave per the encoding engine, not per libc nkf.
- The vendored Ruby half carries one marked deviation: the JRuby loader
  branch is reduced to `require "bigdecimal.so"`.

### `fiddle`

fiddle 1.x ships its own pure-Ruby FFI backend (`lib/fiddle/ffi_backend.rb`,
the JRuby/TruffleRuby path), and that is the fiddle Zeo runs: the backend is
vendored in `crates/zeo-rt/ext/fiddle/` over Zeo's ffi runtime tier (`FFI::Type`,
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
  returns the object's VALUE address, which has no Zeo equivalent.
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

CRuby's coverage extension instruments iseqs as the VM compiles them; Zeo
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

CRuby's TracePoint hooks the VM's trace instructions; Zeo has no VM, so
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
- **`#return_value`, `#parameters`, `#eval_script` and
  `#instruction_sequence` are defined and REFUSE**, with CRuby's own
  `RuntimeError: not supported by this event` (and its `access from outside`
  when no handler is running) -- which is what CRuby answers for a `:line`
  event too. `#self` and `#binding` answer (`tests/tracepoint_self_and_binding.rb`).
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
- `enable(target:)`/`enable(target_line:)` filtering is not implemented.
- **`TracePoint.stat` is empty** — it keys per-VM hook counts on a `RubyVM`
  object Zeo has none of. **`.allow_reentry`** raises CRuby's own
  `No need to allow reentrance.` outside a handler and runs the block inside
  one, but reentrancy suppression stays on either way: a `:line` handler that
  traced itself would not terminate.

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
  framework's `Digest::Class`; Zeo's digest classes are native tables
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
(`docs/EVAL.md` explains how codegen gives just those frames cell
storage). The bounds:

- **A Binding taken inside an INLINE-SPLICED iterator block** (`n.times { |i|
  ... }` and the other bodies Zeo splices into the enclosing Rust scope
  rather than building a `Proc` for) carries that block's own parameters only
  when the enclosing scope is already a binding scope. Elsewhere the spliced
  parameter is a plain per-iteration `let` with no cell to share, so it is
  ABSENT from `local_variables` rather than wrongly bound. Real (escaping)
  blocks and lambdas carry theirs unconditionally.
- **`Binding#irb`** raises `LoadError` — Zeo ships no irb, and a caller can
  rescue that. `#implicit_parameters` is empty and
  `#implicit_parameter_defined?` is false: Zeo compiles `it` and `_1`..`_9` to
  ordinary block parameters, so a Binding carries no separate implicit set.
  That is CRuby's answer for every binding taken outside such a block.
- **A `send` whose method name is COMPUTED** (`m = :eval; send(m, src)`) is
  invisible to static analysis: that binary may carry no compiler at all,
  and the eval gets no scope. A LITERAL `send(:eval, src)` /
  `obj.send(:eval, src)` is fully supported — same locals, same `self` rule as
  CRuby's. `public_send(:eval, …)` is `NoMethodError` in CRuby because
  `Kernel#eval` is private; Zeo does not enforce that visibility, so it
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
is pay-per-use; see `docs/EVAL.md`).

### `Hash.ruby2_keywords_hash`

The marker flag does not exist at run time — Zeo resolves keyword forwarding at
COMPILE time — so `Hash.ruby2_keywords_hash(h)` answers a plain copy and
`.ruby2_keywords_hash?` is false for every Hash, which is CRuby's answer for
any hash that was not marked.

### `GC`

Zeo's heap is `Arc`-refcounted with no tracing collector, so `GC` reports what
is TRUE of it rather than raising: zero collections, no compaction, and
`GC.config` naming the implementation `"refcount"`
where MRI says `"default"`. `GC::OPTS` and `GC::INTERNAL_CONSTANTS` are empty
for the same reason — they describe MRI's build and slot layout. `.enable`/
`.disable` answer the PREVIOUS state, which is what the restore idiom reads,
and `.stress=`/`.auto_compact=`/`.measure_total_time=` record what they were
told so a reader gets its own value back. `GC.latest_gc_info` and
`.latest_compact_info` answer exactly what CRuby answers in a process that has
not yet collected.

`GC.stat` carries `:count` always -- every explicit collection is one, and in
a refcounting heap each is trivially a full one. `:total_allocated_objects`
and `:heap_live_slots` come from the allocation registry, so they are present
exactly while `ZEO_GC=1` is recording; a key absent from the hash reads `nil`,
which a caller can tell apart from a fabricated figure. Every other MRI
statistic describes a slot layout that does not exist here. `stat_heap` stays
empty for the same reason: one size-pooled heap per slot size is an MRI
structure, and there are no heaps here to describe.

`GC.start`, `GC#garbage_collect` and `ObjectSpace.garbage_collect` are one
entry. Each runs the cycle collector when `ZEO_GC=1` armed it and `GC.disable`
has not gated it, then runs finalizers for everything whose last reference has
dropped. **Without `ZEO_GC=1` there is no collector and a cycle leaks**, which
is the default; the flag is a RUN-TIME one, read where the program runs rather
than where it was compiled.

Armed, the pass reclaims every shape `tests/a_cycle_is_reclaimed.rb` builds:
cycles among objects, Arrays, Hashes, Structs, exceptions and value
subclasses, and cycles closed through a compiled `Proc`'s captured local, a
`Range` endpoint, a per-object singleton, or an ivar written on a bare value.

Two of those needed the collector's own rule widened. A `Proc` and a `Range`
are immutable, so the sweep cannot clear what they hold; they report their
edges anyway, because every owner of a reclaimed node is itself reclaimed and
cleared, so the node dies with the pass's own handles. The pass checks it --
its self-check is that a reclaimed node is actually FREED once the pass lets
go, not that its owner count reached one.

The other two were side tables that pinned their owner STRONGLY so an address
could never be reused. The pin is now a `Weak`, which holds the allocation --
all the uniqueness needed -- without holding the value. That removed a
standing leak from the runtime as well as a blind spot in the collector.

zeo also reclaims one shape CRuby does not, in the other direction: a cycle
closed through `Exception#cause`. CRuby keeps the most recently raised
exception reachable from its own VM error slot; a reference count does not.

### `Fiber`

The fiber surface is faithful (`tests/gc_fiber_binding_rows.rb`), with two
bounds:

- **There is no fiber scheduler.** `.scheduler`/`.current_scheduler` answer
  nil, `.set_scheduler(nil)` succeeds and any other argument raises
  `NotImplementedError`, and `.schedule` raises the same `RuntimeError: No
  scheduler is available!` CRuby raises without one.
- **`#backtrace`/`#backtrace_locations` answer `[]` for a fiber other than the
  current one.** A suspended fiber keeps its frames in a saved execution
  context (`crate::ec`) that the backtrace walk cannot enter. A terminated
  fiber answers `[]` in CRuby too.

### `IO#timeout`

`IO#timeout=` records the value and `#timeout` reads it back, but nothing
enforces it: CRuby raises `IO::TimeoutError` when a blocking read outlives the
value, and Zeo's reads block. `nil`, the default, means no timeout in CRuby
either, so a program that never sets one sees no difference.

### `Module` reflection

The constant and method-reflection surface is faithful — `const_get`/`_set`/
`_defined?`, the visibility family, `instance_method`/`public_instance_method`,
`define_method`, `remove_class_variable`, `undefined_instance_methods`,
`set_temporary_name`, `const_missing` and `Module.nesting` are oracle-matched
live (`tests/module_reflection_rows.rb`). Four rows report less, each because
the fact they report is resolved at COMPILE time and left no runtime record:

- **`#const_source_location` answers `[]` for a constant that exists**, `nil`
  for one that does not. `[]` is exactly what CRuby answers for a constant
  defined in C, and every Zeo constant is: codegen resolves a constant path
  statically and the store keeps no file or line.
- **`#autoload` runs its target at the first constant READ**, as CRuby does.
  A literal `autoload :C, "feature"` compiles the target in as a lazy unit and
  keeps the call; the declaration only records. Because a compiled-in class is
  in the dispatch tables from startup, no constant MISS is left to trigger the
  load, so the emitter gates the READ instead (`zeo_rt_autoload_touch`), and a
  miss retries through the same table. `#autoload?` answers the path until the
  target runs and `nil` after, and a constant the target's body ASSIGNS is
  undefined until then.

  Two limits remain. Reading the constant BEFORE the declaration runs loads the
  target, where CRuby raises `NameError` — the compile-time gate has no
  document position. And a target that names a feature which is not compiled in
  is loaded eagerly by the runtime row, because a computed path (including the
  one-argument form `ActiveSupport::Autoload` defines) leaves the compiler
  nothing to gate.
- **`#refinements` is empty.** The compiler mints a `Refinement` module per
  `refine` block and marks it, but records no back-link to the refining
  module. Refined dispatch and `Refinement#target` are unaffected.
- **`Module.used_modules`/`.used_refinements` are empty.** `using` resolves at
  compile time and leaves no runtime activation set.

### `Thread`

A Zeo `Thread` is a REAL OS thread, so the whole scheduling and storage surface
answers for itself: `#join`/`#value`, `#kill`/`#raise` and their checkpoints,
`Thread.stop`/`#run`/`#wakeup`/`#stop?`, `#priority`, `#fetch`, the
thread-variable pair, `#native_thread_id`, `Thread.list` and
`.handle_interrupt` are oracle-matched live (`tests/thread_surface.rb`). Four
rows report less than CRuby's, and all four have the same cause — one OS thread
cannot read another's execution state:

- **`#backtrace`/`#backtrace_locations` answer `[]` for another LIVE thread.**
  The current thread's own frames are real, and a dead thread answers `nil`,
  both as CRuby does; only the cross-thread read is empty.
- **`#priority=` records the number and nothing acts on it.** The kernel
  schedules these threads. CRuby's priority is advisory on the same platforms.
- **`Thread.ignore_deadlock` is stored and read back.** There is no deadlock
  detector here to switch off.
- **`#set_trace_func`/`#add_trace_func` refuse a Proc** with
  `NotImplementedError`, and accept `nil` (there is no hook to clear). This is
  the rule `TracePoint.new` already follows for the events Zeo cannot raise:
  refuse loudly rather than accept a handler that never runs.

### `#source_location` on a row ruby writes in Ruby

CRuby implements 329 method rows across 21 `<internal:>` files -- Ruby it
compiles into its own interpreter -- and each answers a real
`["<internal:nilclass>", 36]`. Zeo implements every one of them in Rust, which
answers `nil`, exactly as ruby answers `nil` for its own C rows.

Only `#source_location` and the `file:line` tail of `Method#inspect` differ.
`#parameters`, `#arity`, `#owner`, visibility and behaviour agree row for row:
the whole-surface census over the 2,594 rows both declare is zero
(`tests/builtin_rows_report_rubys_signature.rb`).

Zeo did vendor CRuby's own `nilclass.rb` and `pathname_builtin.rb` and compile
them, which made these rows answer byte-identically. Measurement retired it:
the compiled Ruby ran **22.7x slower** than the Rust rows (0.359 s against
8.148 s over 200,000 Pathname operations -- and CRuby itself runs the same
source in 1.018 s), cost **3.5x the bytes**, and pulled a 14.48 MB embedded
compiler in for one `eval`. It bought only the signatures, and those are now
spelled in the DSL directly. The `#source_location` residue is a DECIDED
divergence now: `tests/a_row_ruby_writes_in_ruby_names_its_source.rb` (with
its `.divergence` sidecar) pins zeo's `nil` answer and carries ruby's.

### `Pathname`

A core class here, with no `require`, matching ruby 4.0 -- it loads
`pathname.so` before the first line, so 96 instance methods and 3 class
methods are there whatever the program does. Every file test, stat reader and
read/write DELEGATES to the `File` or `Dir` row that implements it, so a
Pathname answers exactly what the same call spelled out answers. Two
divergences:

- **`#find`, `#rmtree` and `.mktmpdir` are present from the start.** ruby adds
  the first two with `require "pathname"` and the third with `tmpdir`; Zeo
  gates whole classes rather than methods, so it answers where ruby raises
  `NoMethodError`, never the reverse. `#find` walks the tree itself rather
  than through `Find`, so `Find.prune` has nothing to prune.

Every one of the 113 rows reports the parameters and arity ruby reports,
including the private helpers -- CRuby writes Pathname in Ruby, so it names
them, and the DSL spells each one. `#source_location` is the one thing that
differs; see the section above.

### `Kernel#block_given?`, `#iterator?`, `#binding`, `#local_variables` through `send`

All four work called directly and through a LITERAL `send`: the compiler
folds each into the caller, where the block and the scope are in hand
(`tests/kernel_scope_intrinsics.rb`).

What stays divergent is a send with a COMPUTED name (`m = :binding; send(m)`).
`block_given?`/`iterator?` need the CALLER's block and `binding`/
`local_variables` need its local scope; neither travels to a method row, and
Zeo's call `Frame` deliberately carries only `(file, line, label)` -- 40 bytes,
pushed on every call -- so widening it would tax every call in the program for
a reflection path almost nothing takes. This is a DECIDED divergence:
`tests/kernel_scope_intrinsics_dynamic_send.rb` and its `.divergence` sidecar
record zeo's answer on purpose.

### `Random::Formatter` carries its whole surface from the start

ruby 4.0 keeps `Random::Formatter` in core with `#rand` and `#random_number`,
and `require "random/formatter"` REOPENS it to add
`hex`/`uuid`/`uuid_v4`/`base64`/`urlsafe_base64`/`random_bytes`/`alphanumeric`/
`choose`/`gen_random`. Zeo gates whole classes rather than individual methods,
so it carries all of them unconditionally and the require is ceremony. A
program that calls `Random.new.hex` WITHOUT the require works here and raises
`NoMethodError` in ruby — the same shape of divergence `require "time"` and
`require "io/console"` already have. Nine methods are affected.

### `Ractor`

Zeo runs no ractors. The six error classes exist so a `rescue
Ractor::ClosedError` in portable code resolves its constant, with CRuby's
exact ancestry (`ClosedError < StopIteration`, the rest under `Ractor::Error <
RuntimeError`); nothing raises them, and `Ractor::RemoteError#ractor` answers
nil. `Ractor`'s own 23 methods stay absent.

### `GC::Profiler`

The switch is real — `enable`/`disable` are remembered and `enabled?` reads
them back — and the readouts are honestly empty: `total_time` is `0.0`,
`result` is `""`, `raw_data` is nil, `report` prints nothing. There is no
tracing collector here to time, so no run has ever been recorded, which is
also what CRuby answers before its first collection.

### `Process::Sys.setresuid` / `.setresgid`

Both are the not-implemented stub everywhere: they take any arguments,
report arity 0, and raise `NotImplementedError: setresuid() function is
unimplemented on this machine`. That is exactly CRuby's own behavior on a
platform without the syscall (macOS among them), and a narrowing on the ones
that have it (Linux, the BSDs), because the stub's arity is 0 while the real
call's is 3 — one declaration cannot report both, and Zeo's arity ledger is
generated on one machine. `Process::Sys.setreuid` and
`Process::UID.change_privilege` reach the same capability.

The rest of `Process::Sys`, `Process::UID` and `Process::GID` is
oracle-matched live in `tests/process_identity_rows.rb`, including every
refusal. `Process::UID.switch`'s saved-id fallback reads an id Zeo seeds on
first use rather than at startup, so a program that moved its effective id
through `Process::Sys` BEFORE ever touching `Process::UID`/`GID` would find
the newer id saved where CRuby kept the original.

## Satisfied faithfully (zeo-bundled gems)

Zeo ships its own copy, intended to match upstream
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

A gem whose real implementation is a C extension Zeo has no built-in for
cannot be compiled. The `require` fails with a message that **names the gem**
rather than looking like an unsupported language feature, and points here and
at the FFI path (see `docs/EXTENSIONS.md`), Zeo's intended escape hatch.
Examples that trigger the named error today: `sqlite3`, `nokogiri`, `pg`,
`mysql2`, `bcrypt`, `nio4r`, `grpc`, `msgpack`, and similar.

## Declined (a CRuby internal, not a missing binding)

Two stdlib extensions expose CRuby's own machinery rather than a library, so
there is nothing for Zeo to bind — reproducing them means rebuilding the
machinery. `require` raises `LoadError`, which is a divergence from ruby, but a
settled one rather than a queued job.

The `LoadError` **names the decision** where Zeo has one to state
(`zeo_abi::declined_feature_reason`), so a caller can tell a decline from a typo or
an unfinished feature.

- **`continuation`** (`Kernel#callcc`) captures and restores the machine
  stack. Zeo compiles to native Rust and has no stack-copying runtime. An
  escape-only `callcc` — enough for an upward jump out of a nested call — is
  reachable, and deliberately not shipped: it would answer the common case
  and silently break re-entry, which is worse than a `LoadError` a caller can
  rescue. CRuby itself prints *"callcc is obsolete; use Fiber instead"* when
  the extension loads, and Zeo ships `Fiber`. Documented as a declined
  divergence in `tests/callcc_is_declined.rb` (a passing step-around test,
  per the gaps README rule that declined divergences don't live in `gaps/`).
- **`ripper`** exposes the reduction event stream of CRuby's `parse.y`. Zeo's
  front end embeds prism, a different parser with a different event model, so
  a binding has nothing to bind to; matching ripper means re-implementing
  CRuby's grammar actions. **`require "prism"` is the answer instead** — the
  real gem's Ruby half over the same prism Zeo itself parses with, which gives
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

`Visitor` belongs to `Color.singleton_class`, exactly as a constant written
there does -- a `class X` IS a constant write with a body. The singleton
methods beside it see it by bare name, `Color.constants` is empty, and
`Color::Visitor` raises `NameError`. All of that now matches.

One thing does not: `Visitor.name`. Real Ruby answers
`"#<Class:0x00007f...>::Visitor"`, an address nobody can reproduce; zeo
answers `nil`, because the qualified name it builds is `#<Class:Color>::Visitor`
and `Module#name` reports nothing for a name no constant path can spell. The
class itself is identical either way.

A body compiled at RUN time (`eval` / `class_eval`) still hands both the
constant and the class to the enclosing module -- there is no compile-time
surrogate to file them on. See
`tests/gaps/an_eval_singleton_body_constant_lands_on_the_module.rb`.

`tests/singleton_body_class_and_self_path.rb` pins the behaviour.

## A top-level `return` inside a required file

`return` at the top level ends the program. In a file the main script
`require`s, real Ruby ends only THAT file's load and carries on in the
requirer; Zeo splices required files into their requirer, so the `return`
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

The ancestry follows too: `M` is recorded on the receiver's singleton chain, so
`Klass.is_a?(M)` and `Klass.singleton_class.ancestors` both report it, and a
repeat `extend` leaves the module at the rank its first one gave it. The same
holds for a per-object `obj.extend(M)`: `obj.is_a?(M)` is true while
`obj.class` and every other instance of that class stay untouched.

The copies carry it the way Ruby's do: an object's `clone` copies the
singleton class (extended modules and `def obj.method` rows) and `dup` drops
it; a class's `dup` and `clone` both keep it, because `rb_mod_init_copy`
clones the singleton class either way. Pinned by
`tests/an_extended_receiver_copies_like_ruby.rb`.

## `zeo bundle install` works; `bundle exec` and `bundler/setup` do not

`zeo gem` works end to end — `zeo gem install --no-document tomlrb` resolves
over HTTPS, verifies both checksums, extracts and installs, with no Ruby on
the machine. It is RubyGems' own binstub, compiled.

**`zeo bundle install` now does too.** From a directory holding a `Gemfile`:

```
$ BUNDLE_PATH=vendor/bundle zeo bundle install
Fetching gem metadata from https://rubygems.org/.
Resolving dependencies...
Fetching tomlrb 2.0.4
Installing tomlrb 2.0.4
Bundle complete! 1 Gemfile dependency, 1 gem now installed.
```

`install`, `update`, `list`, `check`, `show`, `info`, `lock`, `config` and
`--version` all run; `Bundler::CLI.commands.keys` matches CRuby's exactly.
Four defects stood between Bundler and that, each general and each fixed:
a hook in another file could not be ordered against a definition, a hook an
`extend` supplied was installed at the wrong position, a superclass clause
never ran its `autoload`, and a bare `require` matched any loaded path ending
in the same name.

**`bundle exec` and `require "bundler/setup"` still do not**, and both fail
the same way:

```
Gem::GemNotFoundException: can't find gem bundler (= 4.0.16) with executable bundle
```

Bundler asks RubyGems for its own installed gem and binstub so it can set
`BUNDLE_BIN_PATH` for the child process. Zeo carries Bundler as a compiled-in
library, not as a gem in a store, so there is nothing for that lookup to find.
That is a packaging question, tracked on tasks #72 and #73, not a compiler
gap: the command itself dispatches correctly.

To compile against what a bundle resolved, point zeo at the result — the
section below.

## Compiling against an installed gem store

`zeo app.rb --gem-path "$(gem env gemdir)" --bundle-gemfile Gemfile` resolves
the gems your `Gemfile.lock` locked out of the installed RubyGems store — Zeo
reads the Gemfile's lockfile (`Gemfile` → `Gemfile.lock`, `gems.rb` →
`gems.locked`; a path that already ends in `.lock` is read directly). Zeo
consumes Bundler's resolution verbatim — it never resolves, fetches, or builds
extensions — and applies TruffleRuby's `force_ruby_platform`: it uses the
`ruby`-platform (source) gemspec, never a precompiled `.bundle`.

The `GEM_PATH` and `BUNDLE_GEMFILE` environment variables fill in whichever
side the flags left unset (`GEM_PATH` may list several stores, probed in
order). The store activates only when **both** a store and a Gemfile are
known: a flag missing its counterpart is an error, and an ambient `GEM_PATH`
alone never changes a compile (a compile that silently depended on the shell's
gem environment would not be reproducible).

Each locked gem lands in one of three buckets, all recorded in
`zeo-gems.json`:

| bucket | what happens |
|---|---|
| pure Ruby | compiled — added as a require-path root, the majority case |
| name Zeo provides natively (`json`, `psych`, …) | satisfied by Zeo's built-in; the store copy is ignored and the divergence recorded |
| native, unknown to Zeo | **excluded** — recorded with a reason, and a `require` of it fails naming the layout (a locally-built extension, or a precompiled-platform-only install) and pointing at the FFI path |

An excluded gem that the program never `require`s costs nothing but a
disclosure line — an AOT compiler only compiles what a require actually
reaches. `GIT`/`PATH`-source gems (a checkout or a local path in the lockfile)
are not drawn from the store.

This classification is a layout question, not a compile one. It is available
programmatically through `zeo::gem_compat` and `zeo::gem_compat_installed`,
which `gems_require.rs` exercises. To measure whether a gem actually compiles,
compile it and read the `--report` record.

## The per-compile record

`zeo -o app --report app.rb` writes `zeo-gems.json` next to the artifact, one
object per library the program required (`--report=<path>` picks the path):

```json
{
  "json":     {"by": "bundled-gem", "path": "crates/zeo-rt/ext/json/lib/json.rb",
               "diverges": true, "note": "serde_json-backed; not the json gem"},
  "optparse": {"by": "bundled-gem", "path": "gems/optparse/lib/optparse.rb"},
  "base64":   {"by": "builtin-ext", "feature": "base64",
               "diverges": true, "note": "a zeo reimplementation of Base64"},
  "erb":      {"by": null, "excluded": "deferred-require",
               "reason": "required only from a method body, ..."}
}
```

The record is the disclosure channel, and it is opt-in: pass `--report`.
Zeo prints no warning of its own for a substitution — a line on the
program's stderr for every run is noise Ruby never produces.
