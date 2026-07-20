# Library compatibility

How spinel satisfies a `require`, and where its answer is **not** the upstream
gem or C extension. This is prose, not a percentage: each entry carries a
*reason*, because "spinel's `json` is not the `json` gem" is a fact that can
only be stated, never inferred from a score.

Every artifact-producing compile also writes a machine-readable
[`spinel-gems.json`](#the-per-compile-record) recording the same facts for the
libraries a given program actually used. This document is the human-facing
catalogue; that file is the per-program ledger.

## Satisfied, but divergent (a substitution)

spinel provides its own implementation under a name a gem or C extension also
uses. The surface is close, but the backing differs — so an edge case can
diverge, and `spinel-gems.json` marks these `diverges: true`. A compile warns
once per such library (slug `spinel-builtin-substitute`; silence with
`--nowarn=spinel-builtin-substitute`).

| `require` | spinel provides | why it diverges |
|---|---|---|
| `json` | `serde_json`-backed built-in | not the `json` gem; parser/generator options and error subclasses differ |
| `psych` / `yaml` | `yaml-rust2`-backed built-in | not libyaml; tag/anchor and error-position behaviour differ |
| `zlib` | `flate2`-backed built-in | not the `zlib` C extension; stream/checksum surface is partial |
| `digest` | RustCrypto-backed built-in | not the OpenSSL `digest` C extension |
| `openssl` | RustCrypto-backed built-in | a small subset, not OpenSSL |
| `strscan` | spinel `StringScanner` | a reimplementation, not the C extension |
| `stringio` | spinel `StringIO` | a reimplementation |
| `date` | spinel `Date`/`DateTime` | a reimplementation |
| `socket` | spinel `Socket` | a partial reimplementation |
| `base64` | spinel `Base64` | a reimplementation |
| `cgi` | spinel CGI escaping | escape/unescape only |

## Satisfied faithfully (spinel-bundled gems)

Spinel ships its own copy under `gems/<name>/`, intended to match upstream
behaviour. Recorded `by: bundled-gem` with no divergence flag. These are
subsets where noted, not substitutions with a foreign backing.

| `require` | notes |
|---|---|
| `optparse` | `OptionParser` — the common surface |
| `monitor` | `Monitor` + `MonitorMixin` |

## The installed Ruby's own stdlib

A plain-Ruby stdlib library reached over a `-I` load root (the installed
oracle's `rubylibdir`) is spinel-compiled as-is and recorded `by: stdlib-root`.
No substitution is involved — it is the real stdlib source.

## Not available (native gems)

A gem whose real implementation is a C extension spinel has no built-in for
cannot be compiled. The `require` fails with a message that **names the gem**
rather than looking like an unsupported language feature, and points here and
at the FFI path (see `docs/EXTENSIONS.md`), spinel's intended escape hatch.
Examples that trigger the named error today: `sqlite3`, `nokogiri`, `pg`,
`mysql2`, `bcrypt`, `nio4r`, `grpc`, `msgpack`, and similar.

## Compiling against an installed gem store

`spinelc app.rb --gem-path "$(gem env gemdir)" --lockfile Gemfile.lock` resolves
the gems your `Gemfile.lock` locked out of the installed RubyGems store. spinel
consumes Bundler's resolution verbatim — it never resolves, fetches, or builds
extensions — and applies TruffleRuby's `force_ruby_platform`: it uses the
`ruby`-platform (source) gemspec, never a precompiled `.bundle`. Both flags are
required together and are explicit opt-in (a compile that silently depended on
`$GEM_HOME` would not be reproducible).

Each locked gem lands in one of three buckets, all recorded in
`spinel-gems.json`:

| bucket | what happens |
|---|---|
| pure Ruby | compiled — added as a require-path root, the majority case |
| name spinel provides natively (`json`, `psych`, …) | satisfied by spinel's built-in; the store copy is ignored and the divergence recorded |
| native, unknown to spinel | **excluded** — recorded with a reason, and a `require` of it fails naming the layout (a locally-built extension, or a precompiled-platform-only install) and pointing at the FFI path |

An excluded gem that the program never `require`s costs nothing but a
disclosure line — an AOT compiler only compiles what a require actually
reaches. `GIT`/`PATH`-source gems (a checkout or a local path in the lockfile)
are not drawn from the store.

To measure the out-of-the-box number, run `cargo xtask gem-compat` — with no
argument it classifies **every gem installed** in the store (`Gem.dir` by
default), or pass a `<Gemfile.lock>` to measure just its locked subset, and
`--gem-path <dir>` for a different store. It prints a per-gem table and a
headline like *"163/195 store gems usable (84%) — 151 compiled, 12 built-in, 32
native unsupported"*, and writes `conformance/gem-compat.{tsv,md}` with the
native gems grouped by detected layout (the FFI work-list).

## The per-compile record

`spinelc app.rb -o app` writes `spinel-gems.json` next to the artifact, one
object per library the program required:

```json
{
  "json":     {"by": "bundled-gem", "path": "gems/json/lib/json.rb",
               "diverges": true, "note": "serde_json-backed; not the json gem"},
  "optparse": {"by": "bundled-gem", "path": "gems/optparse/lib/optparse.rb"},
  "base64":   {"by": "builtin-ext", "feature": "base64",
               "diverges": true, "note": "a spinel reimplementation of Base64"}
}
```

It is written **by default** — the substitution is silent by nature, so the
record has to already be on disk at the moment a user discovers they need it.
`--no-report` opts out for callers that already know substitutions happen (the
conformance, example, and test harnesses).
