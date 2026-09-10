# The libraries zeo owns

One directory per library, each laid out the way a Rust-backed Ruby gem is
(`rb-sys`'s own template, and `commonmarker`):

```text
<name>/
  <name>.gemspec        where upstream ships one
  lib/<name>.rb         the Ruby half
  ext/<name>/src/*.rs   zeo's Rust, where a Rust gem puts its Rust
```

Two of these follow UPSTREAM's path rather than the template's, because
upstream's is not `ext/<name>/`: `io-console` is
`crates/zeo-rt/ext/io-console/ext/io/console/src/lib.rs`, matching ruby's own
`ext/io/console/console.c`. `build.rs` finds the one `lib.rs` under a
directory's `ext/` whichever shape it uses, and a hyphen in a
gem name becomes an underscore in the module and the cargo feature
(`io-console` -> `io_console`, `ext-io-console`).

`io-console` is also the one library whose METHOD ROWS are not in its own
directory. It adds 34 methods to `IO` itself, and one class owns one
`ruby_class!` table, so the declarations sit in `builtins/io/mod.rs` marked
`gated "io/console"` and forward here. Only `IO::ConsoleMode`, a class of its
own, carries its table in this tree.

`ext/<name>/src/` is the NATIVE half — what CRuby writes in C, written in
Rust here. No directory carries C: a library that wraps one (Oniguruma,
libffi, OpenSSL) reaches it through a `-sys` crate, and `deny.toml` names
the set. `lib/` is the Ruby half, reached by `require "<name>"`, which pulls
the native one in with `require "<name>.so"`. A directory can have either
half alone: `base64` is Rust only, `fiddle` is Ruby only.

A `.gemspec` goes in only where upstream ships one. `socket`, `pty` and
`monitor` are library files in ruby 4.0.6 rather than gems — `monitor.rb`
installs on plain rubylibdir, and neither `ruby/socket` nor `ruby/pty` exists
— so they carry no version, and the compiler finds them by their `lib/`
alone. `crates/zeo/tests/suite/checks/gem_versions.rs` holds that set closed.

Everything here is compiled into `zeo-rt` as a module; `build.rs` declares
them from this tree, so adding a library is adding a directory. See
`crates/zeo-rt/src/ext.rs` for the checklist.

## Provenance

Most Ruby halves are zeo-authored: they exist to declare the exception
classes a feature-gated native class cannot register, and little else. They
are intended to match upstream behaviour, and divergences are documented in
`docs/reference/compatibility.md`.

These files are faithful vendored copies rather than zeo-authored:

| file | from |
|---|---|
| `syslog/lib/syslog/logger.rb` | syslog 0.4.0 (`Syslog::Logger`) |
| `nkf/lib/kconv.rb` | nkf 0.3.0 (`Kconv` and the String patches) |
| `pty/lib/expect.rb` | ruby's own `ext/pty/lib/` (`IO#expect`) |
| `json/lib/json/add/*.rb` (15) | json 2.21.2, verbatim |

`json/add/*` is the additions half: each file teaches one core class
`as_json`/`to_json` and a `json_create` to read it back. They are plain Ruby
with no C behind them, so they are vendored rather than rewritten. What zeo
had to supply for them is the other side of the contract, in `lib/json.rb`
and the native parser: `JSON::JSON_LOADED` (every one of these files checks
it before requiring the library), `JSON::State.from_state` and the rest of
the generator state's field list, and the parser's `create_additions`.

Four libraries are mostly vendored, with marked deviations tagged `zeo:`
in-file:

**`bigdecimal/lib/`** is vendored from the bigdecimal 4.1.2 gem (the version
bundled with ruby 4.0.5) — in 4.x that IS most of the gem (`power`, `sqrt`,
`BigMath`, `to_d`). One deviation: the JRuby loader branch is reduced to
`require "bigdecimal.so"`.

**`date/lib/date.rb`** is the upstream Ruby half verbatim, with three
`zeo:`-marked changes: it opens with `require "date.so"` rather than
`require 'date_core'`, `VERSION` is dropped because zeo's native half already
declares it, and `Date::Error` is ADDED — CRuby defines it in C, and a
feature-gated native class cannot register a constructible exception here.

**`fiddle/`** has no Rust of its own; it rides zeo's ffi. `closure.rb`,
`function.rb` and `version.rb` are verbatim from the ruby 4.0.5 fiddle-1.1.8
gem, and `ffi_backend.rb` — the gem's own pure-Ruby Fiddle over the ffi API,
its JRuby/TruffleRuby path — is vendored with `zeo:`-tagged deviations
(mechanical AOT rewrites, plus aligning the backend's observable messages and
return shapes with the C extension the oracle runs; the list is in
`docs/reference/compatibility.md` `### fiddle`). `lib/fiddle.rb` and the gemspec are
zeo-authored, because upstream's branches on RUBY_ENGINE and builds its
`TYPE_*` constants with a `const_set` loop. The `Importer` DSL files
(`import`/`struct`/`types`/`pack`/`value`/`cparser`) are not vendored —
`Importer` builds methods with `module_eval` on computed strings.

**`prism/lib/`** is vendored from the prism 1.9.0 gem bundled with ruby 4.0.6
— the whole Ruby half, which is where the node classes, the visitors and the
deserializer live. Upstream picks between two backends over one C library (a
C extension on CRuby, FFI everywhere else); both reduce to a handful of
`pm_serialize_*` calls returning a buffer that `Prism::Serialize` decodes in
Ruby. Zeo takes a third branch of the same shape: `lib/prism/zeo.rb` is
zeo-authored, adapted from upstream's `ffi.rb` with its option packing kept
verbatim, and calls the built-in `Prism::Zeo` module over the SAME prism zeo's
own front end parses with. Two removals, marked in `lib/prism.rb`:
`lib/prism/translation/` (the `parser`- and `ripper`-gem adapters, which
subclass third-party gems zeo does not ship) and `lib/prism/ffi.rb` (its
backend, replaced).

**`psych/lib/psych/`** carries six files vendored verbatim from the psych
5.4.0 gem — `class_loader.rb`, `scalar_scanner.rb`, `tree_builder.rb`,
`handler.rb`, `visitors/visitor.rb`, `visitors/yaml_tree.rb` — the pure-Ruby
object-to-node-tree machinery `Gem::Specification#to_yaml` builds its
document with. All six are byte-identical to the gem. `visitors/emitter.rb`
is zeo-authored: upstream's Emitter drives libyaml's event emitter, which
zeo does not carry, so zeo's walks the node tree and writes the text itself
— valid round-trippable YAML, not byte-parity with libyaml's wrapping.
`nodes.rb`, `coder.rb`, `set.rb`, `omap.rb` and `psych.rb` are zeo-authored.

A file not named in this section is zeo's code, under the repository's
MIT OR Apache-2.0. `bigdecimal/`, `fiddle/` and `prism/` carry their
upstream licence text; the other vendored files are ruby/ruby's default
gems, under Ruby's own dual licence, and `THIRD-PARTY-NOTICES.md` at the
root lists them.
