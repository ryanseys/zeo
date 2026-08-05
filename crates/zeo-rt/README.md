# zeo-rt

The runtime for [zeo](https://github.com/ryanseys/zeo), an ahead-of-time Ruby
compiler that emits Rust.

Every binary the `zeo` CLI produces statically links this crate. It implements
Ruby's object model and built-in classes (String with full multi-encoding
support, the numeric tower with Bignum, Hash, Array, Regexp via oniguruma,
Fiber, Thread, and the rest), plus the standard extensions (`socket`, `json`,
`zlib`, `openssl`, `ffi`, and more) behind per-extension cargo features.
The optional `eval-vm` feature links Ruby's prism parser for programs that
call `eval` at runtime; every other program stays parser-free.

You do not depend on this crate directly: the `zeo` compiler builds and links
it for you. Install the [`zeo`](https://crates.io/crates/zeo) CLI instead.

## License

MIT OR Apache-2.0.
