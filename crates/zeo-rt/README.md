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

## Running the tests

Run the unit tests with nextest:

```sh
cargo nextest run -p zeo-rt
```

nextest is REQUIRED, not a preference: the tests mutate process-global state
-- the `GATES` word and the `PATCHED` set in `runtime_meta`, the `REGISTRY`
`OnceLock` in `dispatch`, and other set-once statics. Nextest runs each test
in its own process, which is the whole isolation model. `cargo test -p
zeo-rt` shares one process across tests and is unsupported: a gate one test
arms stays armed for the next, and a second `install_class_registry` panics
("class registry installed twice").

A test that needs real classes installs the core world first:

```rust
crate::dispatch::install_class_registry(ClassRegistry::with_core());
```

`ClassRegistry::with_core()` (bootstrap.rs) registers the builtin classes and
the exception hierarchy at the fixed `zeo-abi` ids -- what a generated
`main()` does before the first program statement. Call it at most once per
test, and only in tests that exercise registry-backed paths; the raise
channels panic loudly registry-less, and several tests assert on exactly that.

## License

MIT OR Apache-2.0.
