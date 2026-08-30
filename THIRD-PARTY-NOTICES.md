# Third-party notices

zeo itself is licensed under [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE).
It redistributes third-party code in four places, each keeping its own
license. This file is a map; the authoritative texts live beside the code.

## Libraries compiled into every binary zeo produces

The zeo runtime (`crates/zeo-rt`) links these C libraries, which its
dependencies vendor and build from source. A program compiled by zeo carries
their code, so a binary you ship carries these notices too.

| Library | License | Vendored by |
|---|---|---|
| [Oniguruma](https://github.com/kkos/oniguruma) | BSD-2-Clause | the `onig` crate (always; zeo's regexp engine) |
| [libffi](https://github.com/libffi/libffi) | MIT | the `libffi` crate (the `ext-ffi` feature) |
| [OpenSSL 3](https://www.openssl.org/) | Apache-2.0 | the `openssl-src` crate (the `ext-openssl` feature) |

Every Rust dependency is permissively licensed by policy -- `deny.toml`
allowlists MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, Unicode-3.0, and
CDLA-Permissive-2.0, and `cargo deny check` enforces it. For the full
resolved list of Rust crates and their licenses, run `cargo deny list`.

## The bundled Ruby standard library (`gems/`)

zeo ships ~70 pure-Ruby libraries -- the standard library it compiles
against, plus rubygems and bundler. `Gemfile.lock` names every one and the
release it comes from; the tiers, and which are third-party versus
zeo-authored, are documented in
[`lib/ruby/UPSTREAM.md`](lib/ruby/UPSTREAM.md) and
[`crates/zeo-rt/ext/UPSTREAM.md`](crates/zeo-rt/ext/UPSTREAM.md). Each
third-party gem keeps its
upstream license file in its own directory: Ruby's `COPYING` + `BSDL` pair
for the Ruby-licensed gems, and the gem's own `LICENSE`/`LICENSE.txt`/
`LICENSE.md` where upstream ships one. All are Ruby-license, 2-clause BSD, or
MIT.

These gems are redistributed in the release tarballs and platform gems (under
`share/zeo/gems/`), and embedded in the `zeo` crate published to crates.io.

## MRI's C API headers (`crates/zeo-rt/cext/include/`)

`crates/zeo-rt/cext/include/` is `ruby/ruby@v4.0.6`'s `include/` tree, vendored
verbatim plus the patch series in `crates/zeo-rt/cext/patches/`. A gem's C
extension compiles against it, so these headers reach any program that loads
one. Ruby is dual-licensed under the Ruby License and 2-clause BSD; upstream's
texts sit beside the tree as
[`crates/zeo-rt/cext/COPYING`](crates/zeo-rt/cext/COPYING) and
[`crates/zeo-rt/cext/BSDL`](crates/zeo-rt/cext/BSDL) -- the same pair that
ships beside every Ruby-licensed gem in `gems/`.
`crates/zeo-rt/cext/README.md` records what the patches change and why.

## The conformance corpus (`tests/spinel/`)

`tests/spinel/` is vendored from the test suite of
[spinel](https://github.com/matz/spinel), MIT licensed, Copyright (c) 2024-
Yukihiro Matsumoto. Upstream's license text is reproduced verbatim at
[`tests/spinel/LICENSE`](tests/spinel/LICENSE), and the vendoring is
documented in [`tests/spinel/UPSTREAM.md`](tests/spinel/UPSTREAM.md).

This corpus is test data. It is not part of any distributed artifact -- not
the release tarballs, not the gems, not the published crates.
