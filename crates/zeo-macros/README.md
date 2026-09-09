# zeo-macros

The proc-macros behind the runtime of [zeo](https://github.com/ryanseys/zeo),
an ahead-of-time Ruby compiler that lowers to Cranelift IR and links a
runtime written in Rust.

`ruby_class! { ... }` and `ruby_module! { ... }` let each built-in Ruby class
in `zeo-rt` declare its methods and constants once. The macro parses the
declaration with the shared `zeo-dsl` grammar and emits the method functions,
lookup tables, constant installers, and a `linkme` registration that the
runtime collects at link time.

You do not depend on this crate directly. Install the
[`zeo`](https://crates.io/crates/zeo) CLI instead.

## License

MIT OR Apache-2.0.
