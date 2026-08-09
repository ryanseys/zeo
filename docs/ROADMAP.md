# Roadmap

Everything Zeo still owes, in one file.

The rule this file lives by: a **divergence** — a program where Zeo answers
differently from ruby — does not belong here. It belongs in
[`tests/gaps/`](../tests/gaps) as an executable XFAIL, so the suite fails the
day someone fixes it. This file holds only **work**: things to build, measure,
or decide.

Where the surface stands: the method census is at **zero rows**
([`METHOD_COVERAGE.md`](METHOD_COVERAGE.md)) — every module, method, constant
and visibility ruby 4.0.6 reaches has a Zeo answer. What remains is
behavioural: the gaps directory, and the build-work below.

Each item states what is measured and what is only suspected. An item that
says "not yet root-caused" means exactly that — start by measuring it, not by
writing the fix.

**Good places to start**, roughly by size: a `to_utf8_lossy` site in
[Correctness](#correctness) (one judgment each, no design work); an AST node
kind in [the mapped tier](#rubyvmabstractsyntaxtree--widen-the-mapped-tier)
(dump the oracle's tree, add one match arm, extend the fixture); an iterator
kind in [lever 2](#2-iterator-inlining-wave-3) (the machinery exists, each
kind is self-contained); a row for `irb`, `minitest` or `openssl` in
[`gems/UPSTREAM.md`](../gems/UPSTREAM.md). [`CONTRIBUTING.md`](../CONTRIBUTING.md)
gives the house rule for all of them: oracle-verified, divergence-documented.

## Divergences (tracked as executable gaps)

[`tests/gaps/`](../tests/gaps) is the real tracker; each file's header carries
the cause, and the suite fails the day a gap starts matching ruby. This file
does not mirror the directory's contents — a table here rotted once already
(it kept naming gaps that had long been promoted). `ls tests/gaps/*.rb` is
the current list.

Declined divergences do not live there either — the gaps README sends them to
a passing test that documents the step-around. Current declined set: `callcc`
(`tests/callcc_is_declined.rb` — needs a restorable machine stack),
`ObjectSpace.each_object` (`tests/objectspace_each_object_is_declined.rb` —
no heap enumeration), and `ripper`: `require "prism"` is a real answer rather
than an absence, so the `LoadError` says so and an e2e test asserts it
(`ripper_is_declined_and_the_load_error_says_so`). A golden could not: goldens
are diffed against the oracle, and ruby loads ripper.

## Correctness

### The lossy-UTF-8 audit

`StrBuf::to_utf8_lossy` renders a string's bytes for display. It is neither
injective (an undecodable byte becomes U+FFFD, so distinct bytes collapse) nor
length-preserving (a Latin-1 `é` renders as two UTF-8 bytes), so any SEMANTIC
use of it — comparing, slicing, matching, or building a result — answers
wrongly for a non-UTF-8 receiver.

**Measured against ruby 4.0.6** with a 269-operation probe (46 String
operations × 6 encodings: UTF-8, ISO-8859-1, ASCII-8BIT, broken UTF-8, KOI8-R,
EUC-JP): **105 divergences, now 27.** UTF-8 receivers never diverged, which is
why the corpus did not catch this.

What is fixed: results now carry the receiver's encoding through the
strip/pad/chop/tr/delete/squeeze family (`str_value_like`), through
`sub`/`gsub`/`split`/`scan` (`reencode_strs` at each exit), and through
`MatchData` (which remembers the encoding its haystack was decoded from).
`chars`/`each_char` slice `char_ranges` instead of decoding; `codepoints`
answers the receiver's own code point; `<=>` compares raw bytes.
`tests/string_encoding_preserved.rb` pins all of it.

The 27 that remain are two classes, both recorded as executable gaps:

- **23 — no validity gate.** Ruby REFUSES most operations on a string whose
  bytes are invalid in its own encoding; Zeo substitutes U+FFFD and answers.
  Needs a `coderange != Broken` check raising `ArgumentError` /
  `Encoding::CompatibilityError`, not an encoding change.
  [`tests/gaps/issue_string_ops_on_broken_encoding.rb`](../tests/gaps/issue_string_ops_on_broken_encoding.rb)
- **4 — symbols carry no encoding.**
  [`tests/gaps/issue_symbol_loses_encoding.rb`](../tests/gaps/issue_symbol_loses_encoding.rb)

**The site count is not the progress metric.** It went 277 → 279 across this
work while divergences fell 105 → 27: the fixes re-encode RESULTS, and several
still read through `to_utf8_lossy` to get there. Two further problems with the
count as written — it includes comments, test asserts and the definition
itself, and it misses `chars()`/`char_vec()`, which are implemented on
`to_utf8_lossy` and are equally lossy, so a site can lower it with no semantic
gain. Driving the count down means moving `sub`/`split`/`scan` onto
`char_ranges` splicing so the decode disappears rather than being re-encoded.

The per-site judgment is unchanged: a DISPLAY path (`to_s`-ish rendering, error
text) keeps the lossy call; a SEMANTIC path must go byte/encoding-aware through
the StrBuf layer. Remaining files by value: `format.rs`, `pack.rs`, `regexp.rs`
haystacks, `io.rs` line reading.

### Raw `\xNN` string literals are not byte-faithful

`"caf\xE9"` reaches the runtime as the UTF-8 bytes for `é` (Latin-1 promotion
in the literal path) where CRuby keeps the single raw byte `0xE9`, UTF-8-tagged
and `valid_encoding?` false. The `"…".force_encoding("ENC")` recognizer already
byte-round-trips its narrow shape; the general literal path needs the same
treatment — a HIR literal representation carrying bytes, not `String`.
Encoding end-to-end tests construct bytes via `chr` until then.

The same family: binary `Digest#digest` bytes still transcode through
pack/Base64/format. The CONCAT and OUTPUT legs are fixed (`StrBuf::push_buf`
byte concat, the raw-byte print family); pack/Base64/format still funnel
through lossy text.

### The block-local scoping fix

[`tests/gaps/block_local_shadows_later_outer.rb`](../tests/gaps/block_local_shadows_later_outer.rb)
records the divergence; the fix is real design work, so it earns a row here
too. Ruby's rule is textual: a name assigned inside a block is block-local
unless the enclosing scope assigned it EARLIER in the source. Zeo's capture
analysis (`codegen/captures.rs::collect_escaping_captures`) is order-blind —
it unions block-referenced names against names the scope assigns ANYWHERE, so
a block-local that shares a name with a LATER outer local becomes a shared
`Captured` cell, and the block's write leaks out. The same misclassification
makes `Ractor.new { e = 1 }` refuse isolation when main rescue-binds an `e`
further down. The fix: the capture set must only admit names whose outer
assignment textually precedes the block. Position data exists in the HIR;
the work is threading it through `collect_locals` and the capture filter
without disturbing the params half (see the `set`-package note in
`captures.rs`).

### Ruby::Box stage 2 — the enabled-mode seams

Stage 1 landed: the census surface, the env gate with CRuby's messages, and
the eval-VM top-owner fix (a dynamic `box.eval("X = 1")` lands on the box's
surrogate, not `Object`). Four seams remain for real enabled-mode isolation,
all designed in the pass-4 plan:

- **Box-keyed runtime method overlay.** `OverlayEntry` grows
  `boxed_methods: FMap<(box_id, Symbol), MethodImpl>` beside the unkeyed map,
  so a box's monkeypatch of a shared builtin stays in the box. Probes check
  `(box, name)` only behind the existing `is_live()` gate — the disabled path
  must stay bit-identical (add no atomic loads).
- **Per-box load bookkeeping.** Stamp `LoadedFile.box_id` at splice time,
  seed `$LOADED_FEATURES` per box, key `feature_already_loaded` by box.
- **Runtime `box.require`/`load`** through the eval VM, searching the box's
  own `$LOAD_PATH`; `wrap:` refused loudly. Today both raise
  `NotImplementedError` naming the compile-time model.
- **`Box.current` as a value.** Today it answers `nil` (the disabled-mode
  answer). The design: a codegen intrinsic answering the enclosing
  `Ctx.box_id` — Zeo's baked box id IS CRuby's "code runs in its defining
  file's box" rule — with the eval VM answering via `Env.box_id`.

### RubyVM::AbstractSyntaxTree — widen the mapped tier

The translator (`builtins/rubyvm_ast.rs`) maps the high-frequency prism kinds
onto parse.y shapes, each pinned against the oracle by
`tests/rubyvm_ast.rb`; everything else answers an honest `:UNKNOWN` leaf.
Widening is mechanical and self-contained per kind: pick a construct
(`case/when`, `begin/rescue`, `def` with rest/kw/block params, `&&=`-family
op-assigns, string interpolation, `yield`, singleton defs), dump the
oracle's tree for a small program, add the `as_*_node` arm, extend the
fixture. Two larger items in the same family: `keep_tokens:` (phase 2 decodes
`pm_serialize_parse_lex` with a prism→parse.y token-symbol table; today
`#tokens` answers `nil`, which is also CRuby's answer when tokens were not
kept), and per-kind `#locations` lists (every node answers its one full-span
Location today; CRuby answers per-kind lists — keyword, operator, …).

### Ractor deadlock detection

A receive that can never be fed blocks forever; CRuby detects the cycle and
raises. Zeo has the pieces (every wait parks on a known port in a known
ractor), so detection is a wait-for graph over the port tables. Unmeasured:
whether CRuby's message (`No live threads left. Deadlock?`-family) is
reachable byte-for-byte from Zeo's model. Related, larger, and owned by the
Ruby::Box overlay work above: globals and cvars are process-shared across
ractors where CRuby raises `Ractor::IsolationError` on non-main access.

### Backtrace frames

Everything else in the 20-case oracle battery matches ruby 4.0.6 verbatim.
These four do not. Per the house rule the first step is writing their gap
files; the frame machinery is the work after that:

- **No C-method frames.** CRuby shows a frame for most C methods —
  `Array#each` between a block and its caller, `Integer#/` at a division's
  line — attributed to the CALLER's file:line. Zeo's builtins are native fns
  that push no frame, so those rows are absent; surrounding Ruby-level frames
  are correct. Fixing this needs a frame push at the dynamic-dispatch boundary
  plus the codegen fast paths. Note `Class#new` is one CRuby itself omits.
- **Arity-error attribution.** CRuby raises "wrong number of arguments" inside
  the CALLEE's frame; Zeo checks arity at the call site, so the innermost frame
  is the caller's.
- **`define_method(:m) { … }` labels.** The literal form desugars to a `def` at
  compile time, so frames say `Foo#m` where CRuby says `block in <class:Foo>`.
- **Class-body execution order.** Class bodies run before top-level statements,
  so a rescued raise in a class body prints before earlier top-level output,
  and the `<main>` frame under a class-body frame reads line 0.

### `pp` prints ENV when StringIO grows an `each_*` method

`pp` printed the whole ENV hash instead of its argument. The trigger is sharp:
adding `StringIO#each_byte` — a plain `def "each_byte"` in `ext/stringio.rs`,
blockless-returns-Enumerator like the existing `each_line` — was enough on its
own to make `tests/pp_pretty_print.rb` print ENV as its FIRST output.
`getbyte`/`readbyte`, added in the same commit, did NOT trigger it; only the
`each_*` one did.

StringIO includes `Enumerable`, and ENV is a hand-rolled native object with its
own lookup. A second `each_*` on an Enumerable-including builtin shifts
something in materialization or the builtin-surface tables such that a
top-level `pp` call resolves against ENV.

Not a gap: the change was reverted, so nothing reproduces it in the tree today,
and the deterministic failure was seen at `-O2` through the CLI while the gap
harness builds `-O0`. To reproduce: re-add the `each_byte` block and run
`cargo nextest run -E 'test(pp_pretty_print)'`. `StringIO#each_byte` is still
missing because of this.

## Gem corpus

**1641 of 1960 probed gems emit Rust** (`cargo xtask gem-probe`, ledger in
`conformance/gem-probe-{compiles,fails}.tsv` + `gem-probe.md`). The probe runs
the compiler front end over a real gem in an isolated view of itself and its
declared dependencies, pinned by the `.gem`'s sha256.

### What that number does and does not say

The ledger's `stage` column names how far up the pipeline a row got, and it is
always read beside `outcome` — `codegen ok` and `codegen lowering-gap` are the
same rung with opposite results. A sweep climbs no further than `codegen`:

| Stage | Claim | Default |
|---|---|---|
| `queued` | the registry names it; nothing has been measured | — |
| `fetch` / `unpack` | the archive resolved and had a `lib/` | — |
| `parse` / `lower` / `analyze` / `codegen` | **how far zeo's front-end passes got** | yes |
| `build` | rustc accepted that Rust and linked a binary | `--build` |
| `run` | the binary executed and exited 0 | `--run` |

The four front-end rungs are zeo's own passes, and a rejection is recorded at
the pass that made it — zeo prints that as the diagnostic's code (`zeo::parse`,
`zeo::lower`, `zeo::analyze`, `zeo::codegen`), so a front-end failure says which
pass refused rather than landing in one bucket.

Reaching `codegen` is deliberately the weakest useful claim. **No rustc runs, no
binary exists, and the gem's own code may not have been compiled at all** — zeo
can decline a unit and defer it to a runtime `LoadError`, so a gem can reach
`codegen` and `build` and still fail to load itself. A sampled build of four
such rows found two that did exactly that. Only `run` distinguishes them.

`--run` executes code downloaded from rubygems, so it is fenced four ways:
named gems only (every bulk selector is refused outright), a second
`--allow-running-untrusted-gem-code` flag, an interactive confirmation that a
non-tty answers `no`, and `sandbox-exec` confinement with no network and no
writes outside a scratch directory.

Read this section as MEASURED, not surveyed: every row below is a diagnostic
the probe actually produced, and the counts move whenever a fix lands. What is
**not** here — anything about gems outside the corpus, or about whether a gem
that emits Rust also RUNS correctly — is unmeasured.

### What remains, by bucket

| Bucket | Rows | Nature |
|---|---|---|
| `lowering-gap` | 180 | Real compiler work; the sub-buckets below |
| `no-entry-point` | 65 | Application gems with no library entry point |
| `missing-dependency` | 30 | A gem the corpus does not carry |
| `no-lib-dir` | 29 | Upstream facts |
| `fetch-failed` | 13 | The sha256 pin disagreeing with rubygems today |
| `compiler-panic` | 1 | A bug, not a limit |
| `timeout` | 1 | A stall, which is the absence of a verdict |

The largest `lowering-gap` sub-buckets, each a single diagnostic:

| Diagnostic | Rows |
|---|---|
| class/module definition inside an undecidable top-level `if` | 42 |
| unknown superclass / class / module (a file the require graph never reached) | 35 |
| `class`/`module` in a position the analyze walk doesn't register | 19 |
| subclassing a built-in type still on the list | 10 |
| superclass mismatch for a class defined twice | 8 |
| the FFI declaration family (literal symbol, `ffi_lib`, scalar type) | 7 |

### Subclassing a builtin: five shapes, not one

Most of the subclassing work resolved into a question about WHERE a subclass
instance's class id can live, and the answer differs per root. The five are
worth knowing before adding a sixth:

1. **Payload root** — the instance is a `ValueSubclass` wrapping a `RubyValue`
   of the root's kind. `Array`/`String`/`Hash`/`Set`/`StringScanner`/
   `StringIO`/`File`/`Enumerator`/`Time`/`Thread`/`Range`. The row cannot
   express the tag, so the wrapper adds it — and which class methods allocate
   through the receiver is recorded per row by the DSL's `allocs` marker,
   because that is where CRuby keeps the same knowledge.
2. **Receiver-honouring root** — the row already allocates through the receiver
   and the value already carries a class id, so there is nothing to wrap.
   `Date`/`DateTime` (an `RDate`) and `Proc` (the class rides in `ProcData`,
   which is what keeps every call-site fast path and `&blk` conversion working).
3. **The native type itself** — the root's constructor already takes the
   receiver class. `ObjectSpace::WeakMap`.
4. **Class-valued instance** — `class X < Module`, whose instances are real
   runtime module ids tagged with an owner class, so `include`, `Module#===`,
   `ancestors` and constant lookup keep working.
5. **Registry-entry only** — the immediates, where the definition is legal and
   there are no instances.

`Regexp` is left out deliberately (subclassing it is vanishingly rare, so the
path would go untested rather than unbuilt); `Socket`/`OpenSSL::SSL::SSLSocket`
are gated on those extensions' maturity.

### Known harness debt

- `write_stub_gemspec` used to overwrite the REAL gemspec inside
  `vendor/gems/<name>/`, and the version stamp then kept those gems from ever
  being re-fetched. The stub goes in the view now, and a cached stub is treated
  as stale so the cache heals itself — but that re-fetch is what surfaces the
  `fetch-failed` rows above, and they need triage: a sha256 mismatch means
  either upstream re-released or the pin was recorded against a different
  artifact, and only one of those is benign.
- A gem's classification is only as good as the last sweep. A contiguous
  ALPHABETICAL band of one diagnostic means a truncated sweep, not a cluster.

## Library

### Vendor test-unit 3.7.8

`require "test/unit"` — `Test::Unit::TestCase`, the assertion set, the runner.
Add a golden. It depends on power_assert for its `assert { }` form: decide
whether to vendor power_assert too or to narrow the surface and say so.

### Promote the rubygems/bundler goldens to the umbrella require

`tests/gems/rubygems.rb` and `tests/gems/bundler.rb` enter at their own files
(`rubygems/version`, `rubygems/requirement`, `rubygems/dependency`,
`rubygems/platform`; `bundler/version`) rather than `require "rubygems"` /
`require "bundler"`, and each header says why.

Both recorded blockers have since landed — `Kernel#binding` (so
`Gem::Specification`'s `eval <<-RUBY, binding, __FILE__, __LINE__ + 1` writer
idiom works) and the method-body require hoist. **Re-probe before planning
anything**; the remaining notes below may be stale too:

- `Gem.operating_system_defaults` — `require "rubygems/specification"` died in
  `config_file.rb:57` calling it, because it is defined in `rubygems/defaults.rb`
  which `rubygems.rb` requires. Root cause was require order, which the hoist
  fix addressed.
- `Gem::Platform.local` reads `Gem.target_rbconfig`, defined in `rubygems.rb`.

When it clears: switch each golden to the umbrella require, restore the
`Gem::Specification` section (a full spec build with runtime and development
dependencies, `full_name`/`file_name`, `to_yaml`/`to_ruby`),
`Gem::Platform.local`, and `Gem::VERSION`/`Gem.ruby_version`/
`Gem.rubygems_version`; give bundler back `LockfileParser` (specs, transitive
deps, platforms, sources, `sections_in_lockfile`), `Bundler::Dependency`
groups/platforms/`to_lock`, `SpecSet`, and the error hierarchy with its exit
codes. Then delete the "entered at those files" headers and drop the
compensating notes from `tests/e2e/gems_vendored.rs`.

## Performance

Measured against CRuby 4.0.6 on 2026-07-31, both timed in the same run:
**1.78× faster over all 58 benchmarks, and 1.23× over the 37 where CRuby takes
0.10 s or more.** 47 of 58 are faster, 11 slower. The difference between the
two aggregates is process startup on 13 sub-50 ms benchmarks. Full table and
method: [`bench/README.md`](../bench/README.md); `bench/baseline.tsv` and
`bench/compile-baseline.tsv` are the banked records.

The staged overhaul that produced this is complete. It took the
compute-bound geomean from 0.86× to 1.23× by replacing the per-call frame
push/pop with a bump pointer, devirtualizing accessors, narrowing the
process-wide deopt latch to four class-keyed flags, compiling a literal
`Struct.new` to a real class, interning the class-ivar and constant sites,
collapsing the per-ivar mutexes into one cell per object and giving it a
single-threaded fast path, sharing duplicate materialized method bodies, and
caching what each dynamic call site resolved to. Compile time fell with it:
`uri_parse_and_build` rustc went 54.9 s → 19.4 s, which closed the rustc-time
lever that used to sit in this list.

### The 11 remaining losses

`life` 0.52×, `rbtree` 0.66×, `linked_list` 0.70×, `splay` 0.74×, `so_lists`
0.74×, `structaset` 0.75×, `inline` 0.83×, `getivar_module` 0.91×, `ao_render`
0.93×, `ruby_xor` 0.95×, `attr_accessor` 0.99×.

**What is measured about them:** they build and tear down object graphs, so
their profiles are dominated by `RubyValue` clone and drop — `so_lists` spends
43% of its samples in `RubyValue::clone` plus `drop_glue` — and by allocation.
The dispatch lever is spent: the inline caches moved this set by 13–16% and did
not carry any of it past 1.00×.

Four levers remain, ordered by expected value. Evidence cites the banked
baselines; anything not yet root-caused says so.

### 1. Tier C: an 8-byte tagged `RubyValue` (largest, deferred)

**Measured payoff** — a tagged word with manual refcounting against today's
24-byte enum, best-of-five on one Apple-silicon laptop:

| operation | 24 B | 8 B | |
|---|---:|---:|---|
| clone + drop, `Arc` arm | 4.019 ns | 3.977 ns | **−1%** |
| clone + drop, `Int` arm | 2.448 ns | 0.344 ns | −86% |
| traverse a 4M-element array | 0.564 ns | 0.189 ns | −66% |
| call returning `Result<V, Signal>` | 4.479 ns | 0.880 ns | −80% |

Two facts decide the sequencing. The reference-count traffic that dominates the
remaining losses does **not** get cheaper — the `Arc` atomic pair survives the
shrink unchanged, so the headline reason to want this is wrong. What does get
cheaper is immediates, array density, and the call return: −3.6 ns on every
Ruby method call, which nothing else can buy, because
`Result<RubyValue, Signal>` has to reach 16 bytes to return in registers and
boxing `Signal` alone measured *worse* (4.91 → 5.15 ns).

**Cost:** manual reference counting in `unsafe` across 91k lines of `zeo-rt`,
where a miscount is a use-after-free rather than a wrong answer. Deferred on
that basis, not abandoned.

**Verify:** the whole corpus, plus the concurrency suite under Miri; the
`ObjectSpace`/`Marshal`/finalizer families are where a miscount would surface
as a wrong answer instead of a crash.

### 2. Iterator inlining wave 3

**Evidence:** waves 1–2 are proven machinery — analyze-mark
(`inline_iter_sites`), guarded splice, the `zeo_rt::iter_inline_ok` runtime
gate. `InlineIterKind` currently covers `times`/`upto`/`downto`/`step`/range
`each`/array `each`/`each_with_index`/`map`/`select`/`reject`/`sum`/hash
`each`. The remaining common kinds still allocate an RProc per call-site
execution and dispatch dynamically.

**Kinds to add:** `inject`/`reduce` (accumulator threading — value-mode like
`sum`, but the block computes the next accumulator), `each_with_object`,
`find`/`detect` (early exit with a value), `min_by`/`max_by`,
`each_slice`/`each_cons` (chunked yields), `count`-with-block, and the in-place
`map!`/`select!`/`reject!` — the in-place family still snapshots, so this is
also a correctness alignment with the live-view decision.

**Verify:** per-kind oracle scratches (especially mutation-during-iteration for
the in-place forms); the enumerable golden family; bench sudoku / nqueens /
life / sort_by.

### 3. Outline the dynamic numeric match arms

**Evidence:** the float arms cost about +5% emitted lines at gem scale (uri
+5.1%, rubygems +5.5%; bm_fib +36% at small-program scale) and the recursion
micros gave back +2–7% — I-cache pressure at `-O2` is the suspect.

**Design:** move the Int-Int and Float/mixed arm ladder into per-op `#[inline]`
zeo-rt helpers so each emitted site shrinks to one call plus the
`send_value_in` fallback; `-O2` static builds inline the ladder back (expect
both the recursion recovery and the line shrink). Semantics must stay EXACTLY
the current arms (tower Flo-lane, `*i as f64` promotion) — one shared source
also kills the arm-vs-row drift risk.

**Gate:** the `-O0` test corpus turns the helpers into real calls; verify suite
wall clock and a hot golden subset do not regress before banking. compile-bench
lines/bytes is the success metric.

### 4. Emission diet round 2

**Candidates:** hash literals still emit per-entry inserts — batch them into
one `zeo_rt::hash_from_pairs(&[…])`, keeping key/value evaluation order
left-to-right and duplicate-key last-wins semantics with its existing parse
warning; pool non-frozen string literals, which allocate twice per evaluation
today (the pool holds the template and each evaluation clones from it).
Remaining match-scaffolding dedupe is mostly covered by lever 3.

Two candidates from this family are **retired, both by measurement**. Coalescing
consecutive `set_line` calls is worth 0.6% of a method call once the frame stack
is a bump pointer, against a real backtrace-correctness risk. Interning the
frame string literals was implemented and reverted: the shared method bodies
removed the duplication it targeted, and it cost +6.6% rustc on uri and +52.9%
on rubygems, because a large `static [Frame; N]` is not free to const-evaluate.

**Metric:** compile-bench lines/bytes down, with no `--dump=rust` semantic
diffs beyond the intended shapes.

### Considered, not scheduled

- **Block & proc call overhead** — send_cfunc_block (1.21×) and
  send_rubyfunc_block (1.68×) are no longer losses, but they are the weakest
  call-path ratios that are not object-graph bound. A leaner block-invoke path
  (no per-call boxing, a direct call for statically-known blocks) would close
  them. Deferred deliberately: it touches the call ABI everywhere, and Tier C
  would rewrite that ABI anyway.
- **Mixed Integer↔Float comparison exactness** — a pre-existing divergence:
  `num_cmp`'s Flo lane converts via `as f64`, lossy past 2^53
  (`9007199254740993 == 9007199254740992.0` answers true; CRuby compares
  exactly and answers false). A fix must update `num_cmp` AND the one inline-arm
  codegen site together. Conformance work, not perf.

## Tooling

### `gem-compat --verify`: compile the gems, don't just classify them

`cargo xtask gem-compat` reports a **resolvability** number, not a
**compiles-successfully** one. Its `pure-ruby` bucket means "Zeo resolved the
gem's `lib/` and would attempt to compile it" — a static classification, never
a real compile. So the headline overstates: many pure-Ruby gems still fail on
stdlib or dependency gaps.

`--verify` should close that the way `stdlib-status` measures stdlib coverage:

1. Classify as today (reuse `zeo::gem_compat` / `gem_compat_installed`).
2. For every `pure-ruby` gem, compile a probe — synthesize a temp `Gemfile.lock`
   from the installed set (`gem_store::installed_as_lockfile` already produces
   exactly this) or reuse the user's lockfile, then run `zeo` on a
   `require "<name>"` probe with `--gem-path <store> --bundle-gemfile <temp>
   --dump=rust`.
   Exit 0 → `compiles`; a clean rejection or a panic → `pure-ruby (fails)`,
   bucketed by stderr (`stdlib_status::reason_bucket` is the existing distiller
   — factor it out and share it).
3. Parallelize like `stdlib-status` (a worker pool over a queue, `zeo` built
   once up front so workers don't race cargo).
4. Report the real number and write per-gem failure reasons to
   `conformance/gem-compat.tsv`, most-common bucket first. That TSV becomes the
   work-list.

**Gotchas.** Opt-in only — the verify pass is 150+ subprocess compiles. The
number WILL drop from the resolvability figure; that is the point. A probe
pulls the gem's whole require graph, so a failure may be a transitive
dependency rather than the gem — keep the bucketed reason so those aggregate.
Add `-I $(ruby -e 'print RbConfig::CONFIG["rubylibdir"]')` to the probe so a gem
needing only a plain-Ruby stdlib file resolves it, matching how `stdlib-status`
reaches the installed stdlib; without it the number understates for a different
reason than it overstates today.

## Docs

- `gems/UPSTREAM.md` has no rows for `irb`, `minitest` or `openssl` — their
  provenance lives only in gemspec header comments.
- `docs/EXTENSIONS.md`'s "adding an extension" step 3 predates `linkme` making
  registration automatic.
