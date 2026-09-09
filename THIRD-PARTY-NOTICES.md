# Third-party notices

zeo itself is licensed under [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE).
It redistributes third-party code in seven places, each keeping its own
license: the C libraries linked into every binary, the fetched Ruby
standard library, the vendored Ruby under `crates/zeo-rt/ext/`, the upstream
test suites under `crates/zeo-rt/gems/`, ruby's own `mkmf.rb`, MRI's C API
headers, and the test corpus. This file is a map; the authoritative texts
live beside the code, except the test corpus's, which is reproduced here.

## Libraries compiled into every binary zeo produces

The zeo runtime (`crates/zeo-rt`) links these C libraries, which its
dependencies vendor and build from source. A program compiled by zeo carries
their code, so a binary you ship carries these notices too.

| Library | License | Vendored by |
|---|---|---|
| [Oniguruma](https://github.com/kkos/oniguruma) | BSD-2-Clause | the `onig` crate (always; zeo's regexp engine) |
| [Prism](https://github.com/ruby/prism) | MIT | the `ruby-prism-sys` crate (always; the parser behind `RubyVM::AbstractSyntaxTree`, the `prism` gem and run-time `eval`) |
| [mimalloc](https://github.com/microsoft/mimalloc) | MIT | the `libmimalloc-sys` crate (always; the global allocator of every compiled program) |
| [libffi](https://github.com/libffi/libffi) | MIT | the `libffi` crate (the `ext-ffi` feature) |
| [OpenSSL 3](https://www.openssl.org/) | Apache-2.0 | the `openssl-src` crate (the `ext-openssl` feature) |

Every Rust dependency is permissively licensed by policy -- `deny.toml`
allowlists MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, Unicode-3.0, and
CDLA-Permissive-2.0, and `cargo deny check` enforces it. For the full
resolved list of Rust crates and their licenses, run `cargo deny list`.

## The bundled Ruby standard library

zeo ships the pure-Ruby standard library it compiles against, plus rubygems
and bundler. None of it is committed: `cargo xtask deps` fetches every gem
at the version the release `Gemfile.lock` names, and the rubygems/bundler
pair at the tag `crates/xtask/rubygems.lock` pins. The tiers are documented
in `crates/zeo/src/gems/bundled.rs`. Each third-party gem keeps its
upstream license file in its own directory: Ruby's `COPYING` + `BSDL` pair
for the Ruby-licensed gems, and the gem's own `LICENSE`/`LICENSE.txt`/
`LICENSE.md` where upstream ships one. All are Ruby-license, 2-clause BSD, or
MIT.

These gems are redistributed in the release tarballs and platform gems (under
`share/zeo/gems/`), and embedded in the `zeo` crate published to crates.io.

## Vendored Ruby under `crates/zeo-rt/ext/`

The extensions' Ruby halves are mostly zeo-authored. The files copied from
upstream are listed, with their versions, in
[`crates/zeo-rt/ext/UPSTREAM.md`](crates/zeo-rt/ext/UPSTREAM.md). Three
carry their upstream licence text beside them (`bigdecimal/`, `fiddle/`,
`prism/`). The rest are files of ruby/ruby's default gems and are under
Ruby's own dual licence (the Ruby License or 2-clause BSD): `psych`'s
pure-Ruby tree-building files, `json/add/*.rb`, `syslog/logger.rb`,
`nkf`'s `kconv.rb`, `pty`'s `expect.rb`, and `date.rb`. Every deviation
from upstream is marked `zeo:` in the file.

## Upstream test suites under `crates/zeo-rt/gems/`

`stringio/` and `strscan/` are zeo's own pure-Ruby ports: their `lib/` and
their gemspecs are zeo-authored, and `strscan/lib/strscan.rb` follows the
structure of ruby/strscan's own `lib/strscan/truffleruby.rb`. Each carries
upstream's test suite verbatim, so the port is held to what the C extension
does rather than to what the port's author expected.

| File | From | License |
|---|---|---|
| `crates/zeo-rt/gems/stringio/test/test_stringio.rb` | ruby/stringio v3.2.0, test/stringio/test_stringio.rb | the Ruby License or 2-clause BSD |
| `crates/zeo-rt/gems/stringio/ruby/ut_eof.rb` | ruby/ruby, test/ruby/ut_eof.rb | the Ruby License or 2-clause BSD |
| `crates/zeo-rt/gems/strscan/test/test_stringscanner.rb` | ruby/strscan v3.1.8, test/strscan/test_stringscanner.rb | the Ruby License or 2-clause BSD |

The `run_pure.rb` drivers beside them are zeo's. The versions are the
ones `Gemfile.lock` pins, so the suite and the port cannot drift apart.
`crates/zeo-rt/gems/UPSTREAM.md` says the same thing beside the code.

## ruby's own `mkmf.rb` (`crates/zeo/tools-lib/mkmf.rb`)

ruby/ruby's `lib/mkmf.rb`, byte-identical, at the `v4.0.6` rev
`crates/zeo-capi/ruby-headers.lock` pins. A gem's `extconf.rb` runs it under
zeo to write the Makefile that a build reads. It must stay byte-identical:
zeo's own additions live separately in
`crates/zeo/src/parse/shims/mkmf_zeo.rb`. The Ruby License or 2-clause BSD,
and it is embedded in the published `zeo` crate.

## MRI's C API headers (`share/zeo/ruby-headers/` in a release)

Nothing of MRI's header tree is in this repository. zeo fetches
`ruby/ruby@v4.0.6`'s `include/` tree (the pin is
`crates/zeo-capi/ruby-headers.lock`) the first time it builds a C extension,
applies the edits `crates/zeo-capi/src/headers/hunks.rs` records, and a release
tarball carries the finished tree pre-seeded so an install needs no fetch. A
gem's C extension compiles against it, so these headers reach any program that
loads one. Ruby is dual-licensed under the Ruby License and 2-clause BSD;
upstream's `COPYING`, `BSDL` and `LEGAL` ride beside the fetched tree, the
same pair that ships beside every Ruby-licensed gem in the store.

## The test corpus (`test/`)

Many of the programs under `test/` were ported from the `test/` suite
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
