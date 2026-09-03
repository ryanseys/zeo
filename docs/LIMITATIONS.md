# Limitations

zeo holds every library name it implements under three **mirror
conditions**, and this file is the ledger those conditions require:

1. **Divergences are ledgered.** Anything zeo answers differently from
   CRuby is written down -- here, in
   [`docs/COMPATIBILITY.md`](COMPATIBILITY.md), or as a committed
   divergence golden under
   [`test/divergences/`](../test/divergences) whose
   `.expected` records ZEO's answer on purpose.
2. **The real thing is the oracle.** Every golden's recorded output comes
   from ruby 4.0.6 resolving the same `Gemfile.lock`, and the pure-Ruby
   ports additionally run their gems' own upstream test suites
   (`crates/zeo/tests/e2e/pure_gems.rs`).
3. **Failure is loud.** Outside the contract, zeo refuses at compile time
   with a named reason, or raises at run time -- never a silent no-op.
   That is the compile contract: CRuby-identical or FAIL.

Four buckets, spinel's taxonomy. An entry moves buckets only with a
commit that says why.

## Fundamental

Differences the architecture chooses; they will not close.

- **Ahead-of-time compilation is the execution model.** There is no
  `RubyVM::InstructionSequence` bytecode to serialize
  (`test/divergences/rubyvm_iseq_serialization.rb`), `ripper` is
  declined (zeo's front end embeds prism), and `continuation` is
  declined.
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
  official C-gem sweep is the API's health gate: `make test-capi`
  builds every locked gem that ships a C extension from its own source,
  runs a smoke program against ruby's recorded answer, and holds the
  rows below (`crates/zeo/tests/fixtures/capi_sweep/XFAIL.json` is the
  ledger; a row that starts passing fails the test until it is
  removed). Families with no honest implementation stay loud stubs.

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

Deliberate answers, each with its reasoning ledgered where it lives.

- **Ruby regular expressions compile and match under Oniguruma** -- the
  one engine, the one CRuby's own Onigmo forked from -- and a pattern it
  refuses is refused. The handful of rows where the two forks answer
  differently are ledgered in `docs/COMPATIBILITY.md` (`### Regexp`).
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
  divergences -- see the encoding section of `docs/COMPATIBILITY.md`.

## Now supported

Formerly on this list; kept so a report against an old version finds
its answer.

- `Monitor#new_cond` / `MonitorMixin::ConditionVariable` (was absent).
- `StringIO` paragraph mode, `readchar`, mode validation, the
  `rb:BOM|UTF-8` open road (were absent or wrong in the native ext until
  the differential matrix caught them).
- `StringScanner` `fixed_anchor: true` (was stored but not acted on) and
  byte-exact `peek` (panicked mid-character).
- `Regexp#match` honors its position argument; `\G` anchors correctly on
  all three engine routes.
- The C-extension route itself: a locked gem's C extension builds from
  source, loads and runs through MRI's fetched headers -- the sweep
  table above says which gems make it all the way.
