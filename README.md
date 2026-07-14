# spinel-rs

A Rust port spike of [spinel](https://github.com/ryanseys/spinel), a
whole-program AOT Ruby-to-C compiler. `spinel-rs` compiles a small subset of
Ruby to Rust source, then shells out to `cargo`/`rustc` to produce a native
binary -- proving out the same architecture in Rust, with genuine open-world
`send`/`define_method`/`method_missing` dynamic dispatch designed into the
object model from the start (something spinel itself never has -- see
[`docs/PORTING_ANALYSIS.md`](docs/PORTING_ANALYSIS.md)).

This is a spike, not a production compiler: it handles exactly the 7 example
programs in `examples/`, chosen to exercise literals, blocks, instance state,
inheritance, and dynamic dispatch end to end.

## Layout

- `crates/spinelc` -- the compiler: `ruby-prism` parse → `Hir` lowering →
  minimal `analyze` → `codegen` (emits Rust source text).
- `crates/spinel-rt` -- the runtime every generated program links against:
  `RubyValue`, `Symbol`, the `ruby_class!` macro, and the `ClassRegistry`/
  `send` dynamic dispatch table.
- `crates/xtask` -- test automation (`cargo run -p xtask -- test`/`regen`).
- `examples/*.rb` + `.expected` -- golden-file fixtures, oracle-verified
  against real `ruby`.

## Usage

```sh
# Compile a Ruby file to a native binary
cargo run -p spinelc -- examples/hello.rb -o /tmp/hello && /tmp/hello

# Print the generated Rust instead of compiling it
cargo run -p spinelc -- examples/dynamic.rb -S

# Run every example through spinelc and diff against real ruby's output
cargo run -p xtask -- test

# Regenerate the .expected golden files from real `ruby`
cargo run -p xtask -- regen
```

## Status

All 7 examples pass. Zero `unsafe` code in `spinelc` or `spinel-rt` (parsing
safety is delegated entirely to the `ruby-prism` crate). See
[`docs/PORTING_ANALYSIS.md`](docs/PORTING_ANALYSIS.md) for the full
feasibility analysis, design rationale, and phased roadmap beyond this spike.

## License

MIT
