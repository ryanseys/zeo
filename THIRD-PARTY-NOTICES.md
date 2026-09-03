# Third-party notices

zeo itself is licensed under [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE).
It redistributes third-party code in five places, each keeping its own
license. This file is a map; the authoritative texts live beside the code,
except the test corpus's, which is reproduced here.

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

## The bundled Ruby standard library

zeo ships ~70 pure-Ruby libraries -- the standard library it compiles
against, plus rubygems and bundler. Nothing is committed: `cargo xtask deps`
fetches every one at the release `Gemfile.lock` names, and the
rubygems/bundler pair at the tag `crates/xtask/rubygems.lock` pins. The
tiers, and which are third-party versus zeo-authored, are documented in
`crates/zeo/src/bundled.rs` and
[`crates/zeo-rt/ext/UPSTREAM.md`](crates/zeo-rt/ext/UPSTREAM.md). Each
third-party gem keeps its
upstream license file in its own directory: Ruby's `COPYING` + `BSDL` pair
for the Ruby-licensed gems, and the gem's own `LICENSE`/`LICENSE.txt`/
`LICENSE.md` where upstream ships one. All are Ruby-license, 2-clause BSD, or
MIT.

These gems are redistributed in the release tarballs and platform gems (under
`share/zeo/gems/`), and embedded in the `zeo` crate published to crates.io.

## MRI's C API headers (`share/zeo/ruby-headers/` in a release)

Nothing of MRI's header tree is in this repository. zeo fetches
`ruby/ruby@v4.0.6`'s `include/` tree (the pin is
`crates/zeo-capi/ruby-headers.lock`) the first time it builds a C extension,
applies the edits `crates/zeo-capi/src/headers/hunks.rs` records, and a release
tarball carries the finished tree pre-seeded so an install needs no fetch. A
gem's C extension compiles against it, so these headers reach any program that
loads one. Ruby is dual-licensed under the Ruby License and 2-clause BSD;
upstream's `COPYING`, `BSDL` and `LEGAL` ride beside the fetched tree, the
same pair that ships beside every Ruby-licensed gem in `gems/`.

## The conformance corpus (`tests/spinel/`)

`tests/spinel/` is vendored from the test suite of
[spinel](https://github.com/matz/spinel), MIT licensed, Copyright (c) 2024-
Yukihiro Matsumoto. Upstream's license text is reproduced verbatim at
[`tests/spinel/LICENSE`](tests/spinel/LICENSE), and the vendoring is
documented in [`tests/spinel/UPSTREAM.md`](tests/spinel/UPSTREAM.md).

This corpus is test data. It is not part of any distributed artifact -- not
the release tarballs, not the gems, not the published crates.

## The test corpus (`test/`)

About 3,100 of the programs under `test/` were ported from the `test/` suite
of [spinel](https://github.com/matz/spinel), zeo's C-emitting predecessor,
and re-recorded against ruby 4.0.6. They sit in the topic directories beside
zeo's own programs; a program's header comment names its origin where it
matters. Spinel's license, reproduced as it requires:

```
Copyright (c) 2024- Yukihiro Matsumoto (matz@ruby.or.jp)

Permission is hereby granted, free of charge, to any person obtaining a
copy of this software and associated documentation files (the "Software"),
to deal in the Software without restriction, including without limitation
the rights to use, copy, modify, merge, publish, distribute, sublicense,
and/or sell copies of the Software, and to permit persons to whom the
Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.
```
