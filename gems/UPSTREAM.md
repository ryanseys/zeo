# Bundled gem provenance

Every directory here is a pure-Ruby gem zeo compiles in when a program
`require`s it (resolved by `parse/loader.rs`; recorded `by: bundled-gem` in
each compile's `zeo-gems.json`). Two origins:

**Faithful vendored copies** of the default/bundled gems shipping with the
oracle Ruby (4.0.5) -- copied verbatim from its installation, versions below,
upstream licenses kept where the gem ships one (all are Ruby-license/2-clause
BSD, compatible with zeo's MIT OR Apache-2.0):

| gem | version | source |
|---|---|---|
| delegate | 0.6.1 | ruby 4.0.5 stdlib |
| english (`English`) | 0.8.1 | ruby 4.0.5 stdlib |
| forwardable | 1.4.0 | ruby 4.0.5 stdlib |
| ostruct | 0.6.3 | ruby 4.0.5 default gem |
| pp | 0.6.4 | ruby 4.0.5 default gem |
| prettyprint | 0.2.0 | ruby 4.0.5 default gem |
| reline | 0.6.3 | ruby 4.0.6 default gem |
| shellwords | 0.2.2 | ruby 4.0.5 stdlib |
| singleton | 0.3.0 | ruby 4.0.5 stdlib |
| timeout | 0.6.1 | ruby 4.0.5 default gem |
| tsort | 0.2.0 | ruby 4.0.5 default gem |

**zeo-authored Ruby halves** of libraries whose native half lives in
`zeo-rt` (`json`, `monitor`, `optparse`, `psych`, `strscan`, `zlib`, `pty`,
`syslog`, `nkf`, `ffi`) -- these are intended to match upstream behaviour;
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

irb is deliberately NOT vendored: every version since 1.15 reads Ruby source
through a Ruby-level Prism (`Prism.lex_compat`, a `Prism::Visitor` subclass),
and zeo exposes no such API -- it embeds prism as a Rust crate, for its own
front end. See `tests/gaps/issue_irb_missing.rb`.

When bumping the oracle Ruby, re-vendor the first table from the new
installation and update the versions here.
