# Bundled gem provenance

Every directory here is a pure-Ruby gem Zeo compiles in when a program
`require`s it (resolved by `parse/loader.rs`; recorded `by: bundled-gem` in
each compile's `zeo-gems.json`). Three origins -- git-sourced, vendored from
the oracle's installation, and zeo-authored:

**Git-sourced.** 24 gems are fetched from their own upstream repository by
`tools/zeo-dev gem`, pinned in `upstream.rb` by `github`, release `tag`
and a full commit `rev` (the reproducible pin; `bundler` also carries a
`subdir`). Their versions live in each gem's own gemspec, their licenses in
each gem's own tree. Currently: `abbrev`, `benchmark`, `bundler`, `csv`,
`drb`, `erb`, `fileutils`, `find`, `ipaddr`, `logger`, `net-ftp`, `net-http`,
`net-protocol`, `net-smtp`, `observer`, `open3`, `racc`, `resolv`, `rubygems`,
`tempfile`, `time`, `tmpdir`, `un`, `uri`.

**Faithful vendored copies** of the default/bundled gems shipping with the
oracle Ruby at the time of vendoring (4.0.5; the oracle now pins 4.0.6 in
`mise.toml`) -- copied verbatim from its installation, versions below, and
every one carrying its upstream license text (all are Ruby-license/2-clause
BSD or MIT, compatible with Zeo's MIT OR Apache-2.0).

A Ruby default gem that ships no license file of its own is covered by Ruby's
own dual license, so those dirs carry Ruby's `COPYING` + `BSDL` pair verbatim
(`delegate`, `English`, `fiddle`, `forwardable`, `prime`, `shellwords`,
`singleton`, `weakref`;
`bigdecimal` carries its own upstream `LICENSE` + `BSDL`). `minitest` is MIT
and reproduces its license in the vendored `README.rdoc`, exactly as upstream
does. The zeo-authored Ruby halves listed further below carry no separate
license: they are Zeo's own code, under the repository's MIT OR Apache-2.0.

| gem | version | source |
|---|---|---|
| date | 3.5.1 | ruby 4.0.6 bundled gem |
| delegate | 0.6.1 | ruby 4.0.5 stdlib |
| english (`English`) | 0.8.1 | ruby 4.0.5 stdlib |
| forwardable | 1.4.0 | ruby 4.0.5 stdlib |
| irb | 1.18.0 | ruby 4.0.6 bundled gem |
| minitest | 6.0.6 | ruby 4.0.6 bundled gem |
| ostruct | 0.6.3 | ruby 4.0.5 default gem |
| pp | 0.6.4 | upstream ruby/pp |
| prettyprint | 0.2.0 | ruby 4.0.5 default gem |
| prime | 0.1.4 | ruby 4.0.6 bundled gem |
| reline | 0.6.3 | ruby 4.0.6 default gem |
| shellwords | 0.2.2 | ruby 4.0.5 stdlib |
| singleton | 0.3.0 | ruby 4.0.5 stdlib |
| timeout | 0.6.1 | upstream ruby/timeout |
| tsort | 0.2.0 | ruby 4.0.5 default gem |
| weakref | 0.1.4 | ruby 4.0.6 stdlib |

`irb/` carries one removal: `lib/irb/ext/tracer.rb` is reduced to a
comment-only file. It hangs off the `tracer` gem, which ruby 4.0.6 does not
ship and Zeo does not vendor.

**zeo-authored Ruby halves** of libraries whose native half lives in
`zeo-rt` (`json`, `monitor`, `openssl`, `optparse`, `psych`, `strscan`,
`zlib`, `socket`, `pty`, `syslog`, `nkf`, `ffi`) -- these are intended to match
upstream behaviour;
divergences are documented in `docs/COMPATIBILITY.md`. Two files inside them
are faithful vendored copies rather than zeo-authored:
`syslog/lib/syslog/logger.rb` (`Syslog::Logger`, from the ruby 4.0.5
syslog-0.4.0 gem, verbatim) and `nkf/lib/kconv.rb` (`Kconv` and the String
patches, from the ruby 4.0.5 nkf-0.3.0 gem, verbatim).

`bigdecimal/` is a third origin: its whole `lib/` tree is vendored from the
bigdecimal 4.1.2 gem (the version bundled with ruby 4.0.5) -- in 4.x that
IS most of the gem (`power`, `sqrt`, `BigMath`, `to_d`) -- with one marked
deviation, tagged `zeo:` in-file: the JRuby loader branch is reduced to
`require "bigdecimal.so"`.

`date/lib/date.rb` is the upstream Ruby half verbatim, with two zeo-marked
changes: it opens with `require "date.so"` rather than `require 'date_core'`
(the same loader idiom `strscan` uses to reach its native half), `VERSION` is
dropped because zeo's native half already declares it, and `Date::Error` is
ADDED -- CRuby defines it in C, and a feature-gated native class cannot
register a constructible exception here.

`fiddle/` follows the same vendored pattern: `closure.rb`, `function.rb`
and `version.rb` are verbatim from the ruby 4.0.5 fiddle-1.1.8 gem, and
`ffi_backend.rb` -- the gem's own pure-Ruby Fiddle over the ffi API, its
JRuby/TruffleRuby path -- is vendored with `zeo:`-tagged deviations
(mechanical AOT rewrites, plus aligning the backend's observable messages
and return shapes with the C extension the oracle runs; the list is in
`docs/COMPATIBILITY.md` `### fiddle`). `lib/fiddle.rb` and the gemspec are
zeo-authored (upstream's branches on RUBY_ENGINE and builds its `TYPE_*`
constants with a `const_set` loop). The `Importer` DSL files
(`import`/`struct`/`types`/`pack`/`value`/`cparser`) are not vendored --
`Importer` builds methods with `module_eval` on computed strings.

`reline/` carries one marked deviation, tagged `zeo:` in-file: `io.rb`
requires `reline/io/ansi` at the top rather than inside `decide_io_gate`,
since a whole-program AOT compile does not load a library that only a method
body requires, and that gate is what every non-dumb terminal goes through.

`prism/` is vendored from the prism 1.9.0 gem bundled with ruby 4.0.6 -- the
whole Ruby half, which is where the node classes, the visitors and the
deserializer live. Upstream picks between two backends over one C library (a
C extension on CRuby, FFI everywhere else); both reduce to a handful of
`pm_serialize_*` calls returning a buffer that `Prism::Serialize` decodes in
Ruby. Zeo takes a third branch of the same shape: `lib/prism/zeo.rb` is
zeo-authored, adapted from upstream's `ffi.rb` with its option packing kept
verbatim, and calls the built-in `Prism::Zeo` module (`ext-prism` in zeo-rt)
over the SAME prism Zeo's own front end parses with. Two removals, marked in
`lib/prism.rb`: `lib/prism/translation/` (the `parser`- and `ripper`-gem
adapters, which subclass third-party gems Zeo does not ship) and
`lib/prism/ffi.rb` (its backend, replaced).

## Choosing a version

Zeo tracks each gem's **latest upstream release**, not the version the oracle
Ruby happens to bundle. The stdlib gems release independently of Ruby itself,
and pinning to a Ruby release would freeze Zeo behind fixes its users want.

`cargo run -p xtask -- gem outdated` prints, per git-sourced gem, the current
pin beside two reference points: what the oracle install resolves, and the
newest upstream tag. Bump with `gem update <name> --tag vX.Y.Z` -- nothing
bumps automatically, so every move is deliberate.

Where a gem ends up ahead of the Ruby it is compared against, expect the
oracle to disagree about that gem's `VERSION`, and record the divergence in
`docs/COMPATIBILITY.md`. `bundler` and `rubygems` ship from one repository and
must move together, on a single shared `tag`/`rev` -- a test enforces it.

Reading a version off the install is easy to get wrong, which is how several
gems here drifted: `gem install` drops newer copies into the same store and
`require` activates the newest, so neither `gem list` nor a require-and-print
probe reports what Ruby actually shipped. For that, read
`<gemdir>/specifications/default/*.gemspec` (Ruby owns that directory), and
for bundled gems (`Gem::BUNDLED_GEMS::SINCE` names them) separate Ruby's
install batch from later user installs by gemspec mtime.

When bumping the oracle Ruby, re-check the first table against the new
installation and update the versions here.
