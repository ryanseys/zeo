# Roadmap

Everything Zeo still owes, in one file.

The rule this file lives by: a **divergence** — a program where Zeo answers
differently from ruby — does not belong here. It belongs in
[`tests/gaps/`](../tests/gaps) as an executable XFAIL, so the suite fails the
day someone fixes it. This file holds only **work**: things to build, measure,
or decide.

Where the surface stands: the method census reached **zero rows** and was
retired — every module, method, constant and visibility ruby 4.0.6 reaches has
a Zeo answer. What remains is
behavioural: the gaps directory, and the build-work below.

Each item states what is measured and what is only suspected. An item that
says "not yet root-caused" means exactly that — start by measuring it, not by
writing the fix.

**Good places to start**, roughly by size: an AST node kind in
[the mapped tier](#rubyvmabstractsyntaxtree--widen-the-mapped-tier) (dump the
oracle's tree, add one match arm, extend the fixture); an iterator kind in
[Performance](#performance) (`arr.each` proved the shape; each remaining kind
is the same guard with a different accumulator); a row for `irb`, `minitest`
or `openssl` in [`gems/UPSTREAM.md`](../gems/UPSTREAM.md).
[`CONTRIBUTING.md`](../CONTRIBUTING.md) gives the house rule for all of them:
oracle-verified, divergence-documented.

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
reachable byte-for-byte from Zeo's model. Related and larger: globals and cvars are process-shared across ractors
where CRuby raises `Ractor::IsolationError` on non-main access. That one is
owned by the box-keyed runtime overlay, tracked as
[`tests/gaps/a_box_write_on_a_shared_class_reaches_main.rb`](../tests/gaps/a_box_write_on_a_shared_class_reaches_main.rb).

## Gem corpus

**185,734 of 195,778 probed gems reach `codegen ok`** — but read the date and
the backend before quoting it. That sweep ran on **2026-08-14**, against the
front end of the **rustc emitter that has since been deleted**, and its
`rust_bytes` column says so. The probe drives `--emit-clif` now; the ledger has
not been re-run. A stratified sample re-probe is owed before the number is
quoted anywhere public.

The ledger is not in git — see [`GEM_TESTING.md`](GEM_TESTING.md). The probe
runs the compiler front end over a real gem in an isolated view of itself and
its declared dependencies, pinned by the `.gem`'s sha256.

### What that number does and does not say

The ledger's `stage` column names how far up the pipeline a row got, and it is
always read beside `outcome` — `codegen ok` and `codegen lowering-gap` are the
same rung with opposite results. A sweep climbs no further than `codegen`:

| Stage | Claim | Default |
|---|---|---|
| `queued` | the registry names it; nothing has been measured | — |
| `fetch` / `unpack` | the archive resolved and had a `lib/` | — |
| `parse` / `lower` / `analyze` / `codegen` | **how far zeo's front-end passes got** | yes |
| `build` | the emitted object linked into a binary | `--build` |
| `run` | the binary executed and exited 0 | `--run` |

The four front-end rungs are zeo's own passes, and a rejection is recorded at
the pass that made it — zeo prints that as the diagnostic's code (`zeo::parse`,
`zeo::lower`, `zeo::analyze`, `zeo::codegen`), so a front-end failure says which
pass refused rather than landing in one bucket.

Reaching `codegen` is deliberately the weakest useful claim. **Nothing is
linked, no binary exists, and the gem's own code may not have been compiled at all** — zeo
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

### Promote the rubygems/bundler goldens to the umbrella require

`tests/bench/rubygems.rb` and `tests/bench/bundler.rb` enter at their own files
(`rubygems/version`, `rubygems/requirement`, `rubygems/dependency`,
`rubygems/platform`; `bundler/version`) rather than `require "rubygems"` /
`require "bundler"`, and each header says why.

Re-probed 2026-08-24: both umbrellas still fail, and the old blockers
(`Gem.operating_system_defaults`, `Gem::Platform.local`) are superseded by
EARLIER ones — the current shapes:

- `require "rubygems"` dies in `specification.rb:136` (`<class:Specification>`)
  with `uninitialized constant Gem::Requirement` — `rubygems/requirement.rb`
  is required by `rubygems.rb` before `specification`, so this reads as the
  positional-require family (a class-body constant read landing before the
  require that defines it was installed).
- `require "bundler"` dies in `kernel_require.rb:37` with
  `undefined method 'gem_original_require' for module Gem` — rubygems'
  `alias_method :gem_original_require, :require` over the builtin `require`
  does not install the alias.

When it clears: switch each golden to the umbrella require, restore the
`Gem::Specification` section (a full spec build with runtime and development
dependencies, `full_name`/`file_name`, `to_yaml`/`to_ruby`),
`Gem::Platform.local`, and `Gem::VERSION`/`Gem.ruby_version`/
`Gem.rubygems_version`; give bundler back `LockfileParser` (specs, transitive
deps, platforms, sources, `sections_in_lockfile`), `Bundler::Dependency`
groups/platforms/`to_lock`, `SpecSet`, and the error hierarchy with its exit
codes. Then delete the "entered at those files" headers.

## Performance

Measured 2026-08-20, the Cranelift backend against the retired rustc baseline:
two waves took it to **+52.3%** (kwargs -87%, `Foo.new` -54%, accessors -68%,
`arr.each` -79%). Against CRuby, compute-bound work still runs at about
**0.71x** — see the README. `bench/baseline.tsv` is the banked record and
`bench/README.md` the method.

**Perf is not a gate.** These levers are recorded so a pass starts from
measurement rather than from a guess, ranked by the gap they close.

| Lever | Gap | What it needs |
|---|---|---|
| `send_rubyfunc_block` | +483% | `obj.ruby_func` on a local whose class analyze KNOWS — a direct compiled-to-compiled call. Needs `analyze::locals`' TyKind map plumbed into `Fx` with the shadowing rules replicated exactly (block params, `for_var_override`, the binding demotion). **A wrong type is a MISCOMPILE, not a missed fold.** A project of its own. |
| the other typed `InlineIterKind`s | so_lists +227%, rbtree +142%, splay +102%, life +99% | `ArrayMap`/`Select`/`Reject`/`EachWithIndex`/`Sum`/`Count`/`Inject`/`HashEach`. `arr.each` proved the shape; each kind is the same guard with a different accumulator. **Statement position builds no result at all, and that alone was more than half the `arr.each` win.** |
| `getivar_module` | +187% | A class-level ivar read is name-keyed per read (a mutex plus two hash lookups). `CivarSite` and `CIVAR_SITE_SIZE` exist and the emitter never emits one. Same `.bss` plumbing as `zeo_callsites`. |
| per-call capi block | fib +113%, tak +132%, tarai +127%, ackermann +123% | Five capi calls per call (`stack_check`, `frame_push`, `set_line`, `check_ints`, `frame_pop`). One per-thread hot block fetched once per function turns push/pop/set_line into stores and `check_ints` into a load-and-branch. Borrow-through params is the other named lever. |
| the boxed local | setivar family +84%, loops_times +94%, nested_loop +97% | `TyKind`-driven unboxing. Same miscompile risk as the first row. |

### Rules about measuring, each learned the hard way

- **A neuter gate measures how much cost EXISTS, not what the design
  RECOVERS.** Frames measured -4.7% neutered and -0.4% recovered.
- **Measure width by WIDENING (one attribute), not by narrowing.**
- **An unfillable inline cache is WORSE than none.** `gcbench` regressed +23%
  until the site remembered its MISS. `new` on a compiled class is served by
  the constructor, not a class-method row, so that site never fills.
- **A second `AtomicBool` beside `is_live` cost 2.6% on dispatch.** Fold a new
  gate into `GATES`' bits.
- **A bench delta needs a CONTROL run.** `bench/baseline.tsv` drifts; +2.6%
  ambient was measured once. Bench HEAD in a worktree. The `%` column is NOT
  the med5 median.
- **`--filter X --update-baseline` rewrites `baseline.tsv` to ONLY X.**
  Rebuild it from `history.tsv` if that happens.

### Considered, not scheduled

- **`-C panic=abort` for `-o` binaries** — investigated 2026-08-11 and retired
  as structurally unsafe, not by measurement: dropping a suspended corosensei
  coroutine works by force-unwinding its stack, and Ruby programs drop
  suspended Fibers and Enumerators routinely. With the `panic_abort` runtime
  linked, that forced unwind aborts the process. Revisit only if fibers ever
  move off unwinding entirely.
- **Mixed Integer/Float comparison exactness** — a pre-existing divergence:
  `num_cmp`'s Flo lane converts via `as f64`, lossy past 2^53
  (`9007199254740993 == 9007199254740992.0` answers true; CRuby compares
  exactly and answers false). A fix must update `num_cmp` AND the one
  inline-arm codegen site together. Conformance work, not perf.

## Docs

- `gems/UPSTREAM.md` has no rows for `irb`, `minitest` or `openssl` — their
  provenance lives only in gemspec header comments.
