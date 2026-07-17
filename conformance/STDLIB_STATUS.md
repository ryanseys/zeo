# stdlib compile status

`cargo run -p xtask -- stdlib-status` -- whether `spinelc` can compile (Ruby -> Rust codegen only, no `rustc`/runtime) each `.rb` in the installed Ruby stdlib `lib`, dropped in via `-I` (no bespoke flag).

- Ruby stdlib: **4.0.5** (`RbConfig rubylibdir`)
- Files swept: **727**
- Compiles: **76** (10.5%)
- Compile-errors: **651** (89.5%)

## Top compile-error reasons

| count | reason |
|------:|--------|
| 166 | `require` is only supported as a top-level statement with a single string-literal argument (spike sc… |
| 59 | expected a constant name or path (e.g. `Foo` or `Foo::Bar`) |
| 40 | cannot load such file |
| 31 | unsupported string interpolation part (spike scope) |
| 26 | `require_relative` is only supported as a top-level statement with a single string-literal argument … |
| 14 | `require "socket.so"`: native (.so/.bundle) features aren't supported (spike scope) |
| 14 | unknown class/module `Bundler` in `Bundler::PubGrub` (must be defined earlier in the file) |
| 14 | unknown class/module `Gem` in `Gem::Molinillo` (must be defined earlier in the file) |
| 7 | unknown class/module `Bundler` in `Bundler::Thor` (must be defined earlier in the file) |
| 7 | unsupported statement in `class << self` (spike scope) |
| 7 | unsupported syntax at "`stty size 2>/dev/null`" (spike handles only what the 7 example programs need… |
| 6 | `require "ripper.so"`: native (.so/.bundle) features aren't supported (spike scope) |
| 6 | expected `*name` as a multi-assignment's splat target |
| 6 | unknown superclass `Psych::Nodes::Node` (must be defined earlier in the file) |
| 5 | `def` with an explicit non-`self` receiver isn't supported yet (spike scope) |
| 5 | panic: unknown class/module `Gem` |
| 4 | `require "digest.so"`: native (.so/.bundle) features aren't supported (spike scope) |
| 4 | `require "openssl.so"`: native (.so/.bundle) features aren't supported (spike scope) |
| 4 | panic: `raise Class, message` requires a literal class name (spike scope) |
| 4 | panic: unknown class `ScanHistory` |
| 4 | subclassing the built-in type `::Hash` isn't supported yet (spike scope, no generated Rust struct ex… |
| 4 | unknown class/module `Bundler` in `Bundler::URI` (must be defined earlier in the file) |
| 4 | unknown class/module `Gem` in `Gem::URI` (must be defined earlier in the file) |
| 4 | unknown superclass `Compiler` (must be defined earlier in the file) |
| 4 | unsupported syntax at "alias __raise__ raise" (spike handles only what the 7 example programs need) |
| 4 | unsupported syntax at "alias gen_random gen_random_urandom" (spike handles only what the 7 example p… |
| 3 | panic: unknown class `Dependency` |
| 3 | panic: unknown class/module `Ripper` |
| 3 | unknown class/module `Gem::SafeMarshal` in `Gem::SafeMarshal::Visitors` (must be defined earlier in … |
| 3 | unknown class/module `Gem` in `Gem::TSort` (must be defined earlier in the file) |

Per-file detail: `conformance/stdlib-status.tsv`.
