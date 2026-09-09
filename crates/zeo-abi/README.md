# zeo-abi

The shared ABI for [zeo](https://github.com/ryanseys/zeo), an ahead-of-time
Ruby compiler that lowers to Cranelift IR and links a runtime written in
Rust.

This is the zero-dependency leaf crate that both `zeo` (the compiler) and
`zeo-rt` (the runtime) depend on. It defines the types they must agree on:
class ids, method tables, extension rows, and the other surfaces the compiler
bakes into generated code and the runtime resolves at execution time.

You do not depend on this crate directly. Install the
[`zeo`](https://crates.io/crates/zeo) CLI instead.

## License

MIT OR Apache-2.0.
