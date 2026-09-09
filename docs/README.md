# Zeo documentation

Four kinds of page, and the kind tells you what to expect from it.

## Tutorials — learning by doing

| Page | |
|---|---|
| [Getting started](tutorials/getting-started.md) | clone, build, run a program, run the suite, write a test |

## How-to guides — a task, start to finish

| Page | |
|---|---|
| [Run the tests](how-to/run-the-tests.md) | profiles, filters, one case |
| [Add a test](how-to/add-a-test.md) | directives, the trailer, fixtures, divergences |
| [Record an answer](how-to/record-an-answer.md) | the oracle, and what `bless` needs |
| [Update the gem versions](how-to/update-gem-versions.md) | bump the lock, refetch, re-record |
| [Add an extension](how-to/add-an-extension.md) | a Ruby library with a Rust half |
| [Build a C-extension gem](how-to/build-a-c-extension-gem.md) | a gem that ships its own C |
| [Measure performance](how-to/measure-performance.md) | the benchmark bank and its baselines |
| [Verify on Linux](how-to/verify-on-linux.md) | the container loop |
| [Cut a release](how-to/release.md) | the tarball, the gem, the crates |

## Reference — the facts, looked up

| Page | |
|---|---|
| [CLI](reference/cli.md) | every verb and flag |
| [Test format](reference/test-format.md) | the directive and trailer grammar |
| [Environment variables](reference/environment-variables.md) | every `ZEO_*` the code reads |
| [Crates](reference/crates.md) | the workspace map |
| [Feature flags](reference/feature-flags.md) | what a build can turn off |
| [Compatibility](reference/compatibility.md) | library by library, against ruby 4.0.6 |
| [Limitations](reference/limitations.md) | what zeo does not do |

## Explanation — why it is built this way

| Page | |
|---|---|
| [Architecture](explanation/architecture.md) | parse, lower, analyze, clif, backend |
| [The backend](explanation/backend.md) | Cranelift, the JIT, the link line |
| [`eval`](explanation/eval.md) | why the compiler is the runtime's evaluator |
| [Gems and the payload](explanation/gems-and-payload.md) | the tiers, and why ruby is only for recording |
| [Testing](explanation/testing.md) | the oracle model, and what the suite refuses to do |
| [Performance](explanation/performance.md) | what the generated code costs, measured |
| [Binary size](explanation/binary-size.md) | what a compiled program weighs |
