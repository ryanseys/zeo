# zeo-rt

The runtime for [zeo](https://github.com/ryanseys/zeo), an ahead-of-time Ruby
compiler that emits Rust.

Every binary the `zeo` CLI produces statically links this crate. It implements
Ruby's object model and built-in classes (String with full multi-encoding
support, the numeric tower with Bignum, Hash, Array, Regexp via oniguruma,
Fiber, Thread, and the rest), plus the standard extensions (`socket`, `json`,
`zlib`, `openssl`, `ffi`, and more) behind per-extension cargo features.
It links Ruby's prism parser, which the parse-only surfaces
(`RubyVM::AbstractSyntaxTree`, the `prism` gem's native half) read; a
run-time `eval` is COMPILED by `zeo` itself, through a seam this crate
declares and the compiler fills.

You do not depend on this crate directly: the `zeo` compiler builds and links
it for you. Install the [`zeo`](https://crates.io/crates/zeo) CLI instead.

## License

MIT OR Apache-2.0.
