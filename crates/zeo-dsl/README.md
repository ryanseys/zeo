# zeo-dsl

The shared `ruby_class!` / `ruby_module!` DSL grammar for
[zeo](https://github.com/ryanseys/zeo), an ahead-of-time Ruby compiler that
emits Rust.

One grammar, parsed with `syn`, serves two consumers: the `zeo-macros`
proc-macro (which expands the DSL into runtime code) and the `zeo` build
script (which projects the compiler's view of the built-in class surface).
Because both sides parse the same declarations with the same code, what the
runtime registers and what the compiler folds against cannot drift.

You do not depend on this crate directly. Install the
[`zeo`](https://crates.io/crates/zeo) CLI instead.

## License

MIT OR Apache-2.0.
