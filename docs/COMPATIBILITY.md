# Library compatibility

## Encoding divergences (zeo-enc)

The encoding engine carries 24 encodings. The single-byte tables
(Windows-125x, ISO-8859-2/-15, KOI8-R) are generated from the ruby 4.0.6
oracle itself, so their mappings -- including which vendor-page bytes have
NO Unicode mapping -- are exact. Known divergences:

- **Shift_JIS mappings are CP932's.** Both the `Shift_JIS` and
  `Windows-31J` rows transcode through encoding_rs's WHATWG `shift_jis`
  table, which matches Windows-31J/CP932. CRuby's strict `Shift_JIS`
  differs in the NEC/IBM extension rows; a program that round-trips those
  extension characters through the `Shift_JIS` row gets CP932 answers.
  Structural validity (what counts as a character) is CRuby-faithful for
  both rows.
- **Big5 pairs that WHATWG maps to two-scalar sequences** (a handful of
  HKSCS combining forms) are treated as unmapped (undefined conversion)
  rather than decoded.
- **String literals with raw high `\xNN` escapes** are not yet
  byte-faithful through the compiler's lowering (see
  docs/todo/stdlib-gaps.md); runtime-constructed bytes (`chr`, IO reads,
  `force_encoding`) are exact.

How zeo satisfies a `require`, and where its answer is **not** the upstream
gem or C extension. This is prose, not a percentage: each entry carries a
*reason*, because "zeo's `json` is not the `json` gem" is a fact that can
only be stated, never inferred from a score.

Every artifact-producing compile also writes a machine-readable
[`zeo-gems.json`](#the-per-compile-record) recording the same facts for the
libraries a given program actually used. This document is the human-facing
catalogue; that file is the per-program ledger.

## Satisfied, but divergent (a substitution)

zeo provides its own implementation under a name a gem or C extension also
uses. The surface is close, but the backing differs — so an edge case can
diverge, and `zeo-gems.json` marks these `diverges: true`. A compile warns
once per such library (slug `zeo-builtin-substitute`; silence with
`--nowarn=zeo-builtin-substitute`).

| `require` | zeo provides | why it diverges |
|---|---|---|
| `json` | `serde_json`-backed built-in | not the `json` gem; parser/generator options and error subclasses differ |
| `psych` / `yaml` | `yaml-rust2`-backed built-in | not libyaml; tag/anchor and error-position behaviour differ |
| `zlib` | `flate2`-backed built-in | not the `zlib` C extension; stream/checksum surface is partial |
| `digest` | RustCrypto-backed built-in | not the OpenSSL `digest` C extension |
| `openssl` | built-in subset | `OpenSSL::Random` + secure compare only; `Cipher`/`PKey`/`SSL` absent |
| `strscan` | zeo `StringScanner` | a reimplementation, not the C extension |
| `stringio` | zeo `StringIO` | a reimplementation |
| `date` | zeo `Date`/`DateTime` | a reimplementation |
| `socket` | zeo `Socket` | a partial reimplementation |
| `base64` | zeo `Base64` | a reimplementation |
| `cgi` | zeo CGI escaping | escape/unescape only |
| `objspace` | always-on `ObjectSpace` rows | see below |
| `io/console` | always-on `IO` rows over `termios(3)` | see below |

### `objspace`

CRuby's `ext/objspace` adds its introspection methods to `ObjectSpace` when
required; zeo's are always present, so the `require` is ceremony (the shape
`io/wait` and `io/console` already have). What answers, and how:

- `memsize_of` computes from zeo's own value representation. CRuby documents
  the figure as implementation-defined and it is — only the shape is portable
  (0 for an immediate, growing with the payload).
- `reachable_objects_from` matches CRuby on everything a Ruby program can see
  (class first, then direct references, immediates dropped), but has no
  counterpart for the internal tier CRuby lists for a Class or a Proc —
  `T_ICLASS`, `T_IMEMO`, method entries — so those answer with their class
  alone.
- `count_symbols` reports the interner total as `immortal_symbol`; zeo never
  frees a symbol, so CRuby's mortal/dynamic/static split has no meaning here.
- `count_nodes`/`count_tdata_objects`/`count_imemo_objects` are empty because
  zero such objects exist, not because they couldn't be counted.
- The `allocation_*` getters answer nil — CRuby's own answer for an object
  allocated outside a trace, which under zeo is every object.
- `memsize_of_all`, `reachable_objects_from_root`, the
  `trace_object_allocations*` family, `dump`/`dump_all`/`dump_shapes`, and
  `internal_class_of`/`internal_super_of` raise `NotImplementedError` naming
  what they'd need (heap enumeration, a root set, an allocation hook, an
  object header, internal classes).

### `io/console`

Its methods are unconditional rows on the `IO` table, so they answer before the
`require` as `io/wait`'s do. The terminal modes (`raw`/`raw!`/`cooked`/
`cooked!`/`noecho`/`echo=`/`echo?`/`getch`/`getpass`/`console_mode`), the
flushes, `winsize`/`winsize=`, `ttyname`, and the cursor/erase escapes are real
`termios(3)`/`ioctl` calls and match CRuby, `Errno::ENOTTY` messages included.
Two divergences:

- `pressed?` and `check_winsize_changed` raise `NotImplementedError`. That is
  CRuby's own behaviour on Unix, message included — they are Windows-only
  there too.
- `IO#cursor` asks the terminal for its position and parses the reply. A
  stream that answers nothing reads as `[0, 0]` rather than hanging.

Before this landed, `IO#winsize` answered `[0, 0]` on a non-terminal where
CRuby raises `Errno::ENOTTY`; it now raises. The corpus expectation that
recorded `[0, 0]` was spinel's, not ruby's, and has been re-oracled.

## Satisfied faithfully (zeo-bundled gems)

Zeo ships its own copy under `gems/<name>/`, intended to match upstream
behaviour. Recorded `by: bundled-gem` with no divergence flag. These are
subsets where noted, not substitutions with a foreign backing.

| `require` | notes |
|---|---|
| `optparse` | `OptionParser` — the common surface |
| `monitor` | `Monitor` + `MonitorMixin` |

## The installed Ruby's own stdlib

A plain-Ruby stdlib library reached over a `-I` load root (the installed
oracle's `rubylibdir`) is zeo-compiled as-is and recorded `by: stdlib-root`.
No substitution is involved — it is the real stdlib source.

## Not available (native gems)

A gem whose real implementation is a C extension zeo has no built-in for
cannot be compiled. The `require` fails with a message that **names the gem**
rather than looking like an unsupported language feature, and points here and
at the FFI path (see `docs/EXTENSIONS.md`), zeo's intended escape hatch.
Examples that trigger the named error today: `sqlite3`, `nokogiri`, `pg`,
`mysql2`, `bcrypt`, `nio4r`, `grpc`, `msgpack`, and similar.

## Compiling against an installed gem store

`zeo app.rb --gem-path "$(gem env gemdir)" --lockfile Gemfile.lock` resolves
the gems your `Gemfile.lock` locked out of the installed RubyGems store. zeo
consumes Bundler's resolution verbatim — it never resolves, fetches, or builds
extensions — and applies TruffleRuby's `force_ruby_platform`: it uses the
`ruby`-platform (source) gemspec, never a precompiled `.bundle`. Both flags are
required together and are explicit opt-in (a compile that silently depended on
`$GEM_HOME` would not be reproducible).

Each locked gem lands in one of three buckets, all recorded in
`zeo-gems.json`:

| bucket | what happens |
|---|---|
| pure Ruby | compiled — added as a require-path root, the majority case |
| name zeo provides natively (`json`, `psych`, …) | satisfied by zeo's built-in; the store copy is ignored and the divergence recorded |
| native, unknown to zeo | **excluded** — recorded with a reason, and a `require` of it fails naming the layout (a locally-built extension, or a precompiled-platform-only install) and pointing at the FFI path |

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

`zeo app.rb -o app` writes `zeo-gems.json` next to the artifact, one
object per library the program required:

```json
{
  "json":     {"by": "bundled-gem", "path": "gems/json/lib/json.rb",
               "diverges": true, "note": "serde_json-backed; not the json gem"},
  "optparse": {"by": "bundled-gem", "path": "gems/optparse/lib/optparse.rb"},
  "base64":   {"by": "builtin-ext", "feature": "base64",
               "diverges": true, "note": "a zeo reimplementation of Base64"}
}
```

It is written **by default** — the substitution is silent by nature, so the
record has to already be on disk at the moment a user discovers they need it.
`--no-report` opts out for callers that already know substitutions happen (the
conformance, example, and test harnesses).
