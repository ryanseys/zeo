# Limitations

What zeo does not do, and why. Everything here is deliberate and
recorded; what is not yet right lives in [`test/gaps/`](../../test/gaps).

- **The cycle collector is opt-in.** Memory is reference-counted, so a
  reference cycle leaks unless `ZEO_GC=1` arms the collector. It is off by
  default because the allocation registry it needs costs a measurable
  share of the benchmark bank. Armed, it runs on its own once the live registry has grown
  enough — nothing has to call `GC.start`. There is no tracing collector:
  the pass reconciles reference counts, because zeo's lowering declares no
  stack maps for Cranelift to build root sets from.
- **C extensions are source-only.** A gem that ships its C source compiles
  and loads — `fast_blank`, `bcrypt`, and `msgpack` all build and answer,
  and bcrypt reproduces a published OpenBSD test vector byte for byte. All
  but 33 of the `rb_*` entry points a gem can link against are answered, and
  each of those 33 raises with its reason (they boot an interpreter, or
  expose a representation Zeo does not have). A gem shipping a *precompiled*
  `.so` never loads: that object is CRuby's ABI. Autotools and
  `mini_portile` builds of a vendored C library are out of scope;
  `have_library` works. A C extension together with a second Ruby thread
  arms the GVL; a single-threaded program keeps the lock-free path.
- **`Ruby::Box` isolation is partial.** A box works at compile time and at
  run time — `Ruby::Box.new`, `box.eval`, `box.require`, and `Box.current`
  all answer — and it isolates constants and globals. It does not yet
  isolate a monkeypatch of a shared builtin (which reaches main), and a box
  cannot require a feature the whole-program compile already spliced. Both
  are tracked in [`test/gaps/`](../../test/gaps).
- **Four extensions are partial** — `coverage`, `nkf`, `openssl`,
  `TracePoint`. See [Add an extension](../how-to/add-an-extension.md).
- **The Cranelift backend's performance pass is not finished.** Correctness
  is complete; code quality is not. [Performance](../explanation/performance.md)
  has the measured numbers, program by program.

---

zeo holds every library name it implements under three **mirror
conditions**, and this file is the record those conditions require.

The tree tracks no C of its own, with one gated exception: three files
under `crates/zeo-capi/csrc/` carry the variadic C API entry points
(`rb_raise`, `rb_funcall`, `rb_scan_args`) that Rust cannot write before
`c_variadic` stabilises in Rust 1.99. `checks::no_c` holds that set exact.

1. **Divergences are written down.** Anything zeo answers differently from
   CRuby is written down -- here, in
   [`docs/reference/compatibility.md`](compatibility.md), or as a committed
   divergence golden under
   [`test/divergences/`](../../test/divergences) whose `__END__` trailer
   records ZEO's answer on purpose.
2. **The real thing is the oracle.** Every golden's recorded output comes
   from ruby 4.0.6 resolving the same `Gemfile.lock`, and the pure-Ruby
   ports additionally run their gems' own upstream test suites
   (`crates/zeo/tests/suite/api/pure_gems.rs`).
3. **Failure is loud.** Outside the contract, zeo refuses at compile time
   with a named reason, or raises at run time -- never a silent no-op.
   That is the compile contract: CRuby-identical or FAIL.

Four buckets. An entry moves buckets only with a commit that says why.

## Fundamental

Differences the architecture chooses; they will not close.

- **Ahead-of-time compilation is the execution model.** There is no
  `RubyVM::InstructionSequence` bytecode to serialize
  (`test/divergences/rubyvm_iseq_serialization.rb`), `ripper` is prism's
  Ripper translation rather than parse.y's event stream, and
  `continuation` is declined.
- **Threads run in parallel without a global VM lock.** The GVL exists
  only for C extensions: one arms it the moment a second Ruby thread
  exists, and nothing else does -- `the_zeo_native_stdlib_never_arms_the_gvl`
  pins that the whole zeo-native stdlib never arms it, and the C-gem sweep
  pins that a single-threaded program with a C extension does not either. Scheduling interleavings and
  data-race observability therefore differ from CRuby; `ZEO_GVL=1`
  restores CRuby's handoff for differential debugging.
- **Object identity is not an address.** Allocation tracing and
  `object_id` patterns that decode addresses differ
  (`test/divergences/object_identity_and_allocation_tracing.rb`).

## Partial, relaxable

Real gaps with a named road; each closes when its road is walked.

- **The C-API is third-party insurance, not the stdlib's default.**
  Every stdlib name is served by zeo's own Rust or Ruby; official C gems
  compile and load through the C API (`zeo-capi`, Rust over the runtime,
  with MRI's headers fetched at the first build) as an explicit
  opt-in (a lockfile + store override, or `ZEO_DISABLE_BUILTIN`). The
  official C-gem sweep is the API's health gate: `api::capi_sweep`
  builds every locked gem that ships a C extension from its own source,
  runs a smoke program against ruby's recorded answer, and holds the
  rows below (`crates/zeo/tests/fixtures/capi_sweep/XFAIL.json` is the
  record; a row that starts passing fails the run until it is
  removed). Nothing runs the sweep for you -- see
  [Run the tests](../how-to/run-the-tests.md#the-tests-that-build-a-real-gem).
  For a gem OUTSIDE the lock, `cargo xtask test-gem <gem>` asks the narrower
  question of whether zeo can compile it at all. Families with no honest implementation stay loud stubs.

  | Gem | Through its C extension | Wall |
  |---|---|---|
  | json, nkf, io-console, prism, syslog | **passes** | -- |
  | bigdecimal | fails at `Init` | `rb_define_class(.., rb_cNumeric)` sees `false` |
  | date | loads; `>>` and `strftime` wrong | a stray `ractor` send; empty strftime |
  | debug | does not load | `rb_iseq_code_location` (iseq internals; closed) |
  | digest | loads; then raises | `Digest::Base cannot be directly inherited` |
  | erb | loads; SIGSEGV | first `ERB::Escape.html_escape` |
  | fiddle | fails at `Init` | `rb_memory_view_register` is a stub |
  | openssl | fails at `Init` | `rb_require("digest")` leaves `Digest` undefined |
  | psych | fails in its Ruby half | `Psych::Config` unresolved on the store road |
  | rbs | every answer right; SIGSEGV at exit | TypedData teardown |
  | stringio, zlib | fail at first call | an Integer the boundary refuses to hand over |
  | strscan | fails at first scan | `rb_reg_onig_match` (refused by decision) |

  racc is excluded (zeo declines its C accelerator by design) and resolv
  (its only extension is Windows-only).
- **A packaged frame's `__FILE__` and backtraces** show the virtual
  `/zeopkg/...` spelling, not the install path (revisits when binding
  virtual roots to store paths lands).
- **rexml does not compile** (a builtin-superclass shape the CLIF
  backend cannot lower yet), which also blocks running test-unit-based
  suites UNDER zeo; those suites run under CRuby against the pure trees
  instead.

## By design

Deliberate answers, each with its reasoning recorded where it lives.

- **Ruby regular expressions compile and match under Oniguruma** -- the
  one engine, the one CRuby's own Onigmo forked from -- and a pattern it
  refuses is refused. The handful of rows where the two forks answer
  differently are listed in `docs/reference/compatibility.md` (`### Regexp`).
- **`racc` runs its own pure-Ruby runtime**: the C accelerator is a pure
  speed-up with a complete in-gem fallback, so zeo declines the build
  and `Racc_Runtime_Type` answers `"ruby"`
  (`test/divergences/racc_declines_the_c_accelerator.rb`).
- **`Readline` IS `Reline`** -- ruby's own arrangement since 3.3, stated
  directly instead of probing for a C readline that cannot exist here.
- **`nkf` implements the kconv subset** over the runtime's own encoding
  engine, not all of NKF's option surface.
- **The YAML emitter wraps long lines differently from libyaml**
  (`test/divergences/yaml_a_narrow_line_width_folds_differently.rb`);
  the parsed VALUES round-trip identically.
- **JSON error messages are lossy about invalid bytes**
  (`test/divergences/json_message_bytes_are_lossy.rb`), and parser
  nesting is bounded by an explicit limit rather than the machine stack
  (`test/divergences/json_nesting_is_bounded_by_the_stack.rb`).
- **The native StringIO copies its buffer** (a `Vec`, not the caller's
  String), so mutating the original after `StringIO.new(s)` is invisible
  to it; the pure-Ruby port (`crates/zeo-rt/gems/stringio/`) shares the
  real object and is the reference for that behavior.
- **`MonitorMixin` initializes lazily** on first use instead of imposing
  CRuby's `initialize`-chain contract on every including class; CRuby
  itself uses the lazy shape in `new_cond`.
- **Shift_JIS maps through CP932**, and the other encoding-table
  divergences -- see the encoding section of `docs/reference/compatibility.md`.
