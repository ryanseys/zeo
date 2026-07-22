# zeo

**An ahead-of-time Ruby compiler.** zeo compiles a whole Ruby program to a
single native executable: it parses with [Prism] (CRuby's own parser), runs
whole-program analysis over a typed IR, emits Rust, and links a precompiled
runtime. No interpreter startup, no JIT warmup, no Ruby installation needed on
the machine that runs the binary.

```console
$ cat hello.rb
puts [1, 2, 3].map { |x| x * 2 }.sum
$ zeo hello.rb -o hello
$ ./hello
12
```

[Prism]: https://github.com/ruby/prism

## Compatibility, honestly

zeo's compatibility contract is **oracle-verified behavior, with every
divergence documented** — never silent wrongness. The conformance suite
compiles ~2,300 golden-output programs and diffs them against real `ruby`
(currently 4.0.5); the scoreboard lives in
[`conformance/SCOREBOARD.md`](conformance/SCOREBOARD.md) (**93%** passing and
climbing). Where zeo substitutes its own implementation for a gem or C
extension (`json`, `psych`, `zlib`, …), the compile says so — a warning at
build time and a machine-readable `zeo-gems.json` ledger next to every
artifact. The catalogue of substitutions and known divergences is
[`docs/COMPATIBILITY.md`](docs/COMPATIBILITY.md).

The north star: `zeo` compiles **rubygems and bundler out of the box**.

## How it works

```
foo.rb ──prism──▶ HIR arena ──analyze──▶ typed classes/MRO ──codegen──▶ Rust ──rustc──▶ foo
                  (whole program:            (compile-time             (quote! +        (links the
                   requires spliced           method tables,            prettyplease)    precompiled
                   at compile time)           type inference)                            zeo-rt runtime)
```

- **Whole-program**: `require` is resolved at compile time; the entire program
  (including bundled pure-Ruby gems under `gems/`) is one compilation unit.
- **Two dispatch paths**: statically-resolvable calls compile to direct Rust
  calls; everything else goes through the runtime's real method registry
  (which also serves `define_method`, singletons, and runtime-minted classes).
- **A real runtime** (`zeo-rt`): CRuby-faithful numeric tower
  (Integer/Bignum/Rational/Complex), encodings as bytes + interpretation,
  Fibers on real stack-switching coroutines, **truly parallel threads**
  (real OS threads, no GVL by default — `ZEO_GVL=1` opts into CRuby-style
  serialized scheduling), exceptions as `Result` propagation —
  `#![forbid(unsafe_code)]` outside the FFI/syscall shims.

## Building from source

Requirements: Rust ≥ 1.87 (see `rust-version`), a C compiler (for the vendored
Prism and Oniguruma), and — only for running the conformance suite against the
oracle — a real Ruby matching `conformance/SCOREBOARD.md`'s version.

```console
$ git clone https://github.com/ryanseys/zeo && cd zeo
$ cargo build --release -p zeo
$ target/release/zeo yourprogram.rb -o yourprogram
```

Development workflow:

```console
$ cargo test --workspace                          # unit + e2e suites
$ cargo run -p xtask -- test                      # examples/ golden suite
$ cargo run -p xtask -- conformance run --smoke   # 153-test quick tier
$ cargo run -p xtask -- conformance run           # the full corpus
$ cargo run -p xtask -- bench                     # golden-output benchmarks
```

## Status

Experimental and moving fast. The large structural pieces in flight are
tracked in `docs/` (object-model completion, the always-linked eval VM,
Onigmo-backed regex, Ruby::Box isolation). Issues and contributions
welcome — see [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Dual-licensed under either of [MIT](LICENSE-MIT) or
[Apache License 2.0](LICENSE-APACHE), at your option. Unless you explicitly
state otherwise, any contribution intentionally submitted for inclusion in
zeo by you, as defined in the Apache-2.0 license, shall be dual licensed as
above, without any additional terms or conditions.

Vendored components keep their own (compatible) licenses — see
`gems/UPSTREAM.md`, `conformance/corpus/UPSTREAM.md`, and
`bench/UPSTREAM.md` for provenance.
