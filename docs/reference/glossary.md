# Glossary

The words this documentation uses in a sense of its own. Ruby's own
vocabulary is not repeated here.

## The test corpus

**Oracle** — the pinned `ruby` that decides what a program's answer is.
`.ruby-version` names the version, and `ZEO_RUBY` names the binary. Only
recording an answer needs it; building and running the suite do not.

**Golden** — one `.rb` file under `test/`, and the answer recorded at the
bottom of it. The file is the whole test: run the program, compare stdout,
stderr and the exit status byte for byte.

**Trailer** — the recorded answer itself, everything after the `__END__`
line. `cargo xtask bless` writes it from the oracle.

**Directive** — a `#@` line in a golden that changes how it runs: an
argument, stdin, an environment variable, a platform gate. The full grammar
is in [the test format](test-format.md).

**Gap** — a program under `test/gaps/` that zeo does not get right yet. Its
verdict is inverted: the suite goes red the day it starts matching ruby,
which is what stops a fixed gap from sitting unnoticed.

**Divergence** — a program under `test/divergences/` that zeo answers
differently on purpose. Its answer is recorded from zeo, not from ruby, and
its header says why.

**Milestone** — a program under `test/milestones/` that requires a whole
library graph (rubygems, bundler) rather than testing one behaviour. They
are slow, so they run under the `full` profile only.

**Leg** — how a golden is run. The JIT leg compiles in process and runs it;
the AOT leg links a real binary with `cc` and runs that. `test/aot/` runs
both.

## The compiler and runtime

**Payload** — everything a zeo installation ships beside the binary: the
bundled standard library, the runtime archive, the headers pin. The binary
finds it by looking at `bin/../share/zeo`, and `ZEO_HOME` overrides that.

**Tier** — where a `require` finds a library. Zeo's own Rust
implementations come first, then the pure-Ruby halves it ships, then a real
RubyGems store the lockfile names.

**Extension** — a standard-library module zeo implements itself: a Ruby half
under `crates/zeo-rt/ext/<name>/lib/` and a Rust half beside it. Each has an
`ext-*` cargo feature.

**Ownership ledger** — the runtime's record of which emitted value owns
which allocation. `ZEO_RT_LEAKCHECK` balances it at exit; a non-zero balance
is a leak or a double-consume in the generated code, not in a user program.

**Box** — `Ruby::Box`, Ruby 4.0's namespace isolation. Off unless `RUBY_BOX=1`
is set, as in CRuby.

**Class surface** — the builtin methods and constants the compiler folds
against, projected from the runtime's own declarations by `crates/zeo/build.rs`.
Both sides parse the same text, so what the runtime registers and what the
compiler believes cannot drift.
