# Testing Zeo against real gems

Zeo measures gem support in two places, and they answer different questions.

| | Question | Command |
|---|---|---|
| **`gem-compat`** | Would this gem's LAYOUT resolve? | `cargo xtask gem-compat <Gemfile.lock>` |
| **`gem-probe`** | Does the compiler actually accept it? | `cargo xtask gem-probe <name>` |

The distinction matters. `gem-compat` never runs the compiler, so it classifies
concurrent-ruby as "pure-ruby, 100% resolvable" — a gem that does not compile.
Only `gem-probe` runs the front end, so only its `compiles` means compiles.
Do not quote a resolvability percentage as a compatibility number.

## Probing a gem

```console
$ cargo xtask gem-probe kramdown          # newest release
$ cargo xtask gem-probe rake 13.3.1       # a pinned version
$ cargo xtask gem-probe --corpus          # every gem in the corpus file
$ cargo xtask gem-probe --index           # every gem on rubygems.org
$ cargo xtask gem-probe --index --limit 500
$ cargo xtask gem-probe --all             # re-probe at recorded versions
$ cargo xtask gem-probe --all --check     # as above; fail if one regressed
$ cargo xtask gem-probe --corpus --refresh
$ cargo xtask gem-probe kramdown --no-deps
```

The gem does not need to be installed. Nothing here needs Ruby.

### It resumes, and it writes as it goes

The ledger is rewritten after **every** gem, not once at the end, and a run
skips whatever the ledger already holds. Both matter at registry scale: the
compact index lists around 200,000 gems, so a full sweep is measured in hours
and being interrupted is the normal case, not the exceptional one.

```console
$ cargo xtask gem-probe --index --limit 500     # a slice
^C                                             # keep every result so far
$ cargo xtask gem-probe --index --limit 500     # the NEXT 500, not the same ones
```

`--all` re-probes everything in the ledger at its recorded version, and
`--refresh` re-probes a selection that resume would otherwise skip. Use those
after a compiler change; use plain `--index`/`--corpus` to extend coverage.

## How a probe runs

Four stages, and only the first touches the network:

```
resolve   name [version]      -> an exact version        (rubygems JSON API)
fetch     the .gem            -> vendor/gems/<name>/     (cached, gitignored)
probe     require "<entry>"   -> an Outcome              (front end + codegen)
record    the Outcome         -> conformance/gem-probe.{tsv,md}   (committed)
```

Probing an unpacked tree needs no network and is deterministic, so a ledger row
reproduces from its recorded version alone.

Three details are load-bearing rather than incidental:

- **A gem unpacks to `vendor/gems/<name>/`, not `<name>-<version>/`, and its
  gemspec is replaced with a stub.** Zeo checks a gemspec's name against its
  directory name, and parses gemspecs statically — so the computed
  `s.version = Foo::VERSION` that most real gems use is rejected. This is the
  same layout `gems/` uses, so a probe exercises the loader path bundled gems
  take.
- **Each probe sees only its own gem and that gem's dependencies.** Probing
  against the whole cache made a verdict depend on what else had been fetched:
  kramdown reported one gap alone and a different one once
  kramdown-parser-gfm sat beside it.
- **The entry point comes from the files the gem ships, never from its name.**
  `activerecord` ships `active_record.rb`. Requiring a feature that does not
  exist is not an error in Zeo — an unresolvable `require` lowers to a runtime
  `Kernel#require` — so codegen succeeds having compiled none of the gem. That
  bug reported every Rails gem as `compiles`. A gem whose entry point cannot be
  identified records `no-entry-point` instead of a false pass.

## Outcomes

| Outcome | Meaning |
|---|---|
| `compiles` | The front end accepted the gem's entry point. |
| `lowering-gap` | Zeo cannot lower a construct. The detail names it. |
| `compiler-panic` | Zeo panicked. A bug, not a stated limit. |
| `native-extension` | The gemspec declares C extensions. |
| `missing-dependency` | A `require` reached outside the gem and its deps. |
| `no-lib-dir` | No `lib/`. Usually a meta-gem, like `rails`. |
| `no-entry-point` | A `lib/`, but no file this gem's name could name. |
| `fetch-failed` | No such gem, or the registry could not be reached. |

## What `compiles` does and does not mean

It means the gem's conventional entry point reached code generation. It does
**not** mean the gem works.

- Nothing is linked or run. `--run` does that for one gem at a time.
- The entry point is whatever the gem calls its own. Some gems make that the
  whole library; `actionpack`'s `action_pack.rb` is one line that requires a
  version file. `compiles` is a weak signal for the latter.
- A gem that compiles can still fail at run time on behaviour Zeo diverges on.

Read a `compiles` row as "the compiler has no objection to this source", and
nothing more.

## Growing the corpus

[`../conformance/gem-probe-corpus.txt`](../conformance/gem-probe-corpus.txt) is
the transitive runtime-dependency closure of the 200 most downloaded gems on
rubygems.org, resolved from the registry rather than guessed. Add a name and run
`--corpus`.

The corpus is the curated, reviewable list. `--index` goes wider, to every gem
the registry knows — useful for finding constructs nothing in the corpus
exercises, and the results simply accumulate in the same ledger.

Rankings come from [rubygems.org/stats](https://rubygems.org/stats), which
covers the top 100; beyond that the search API reports per-gem download counts.
The [weekly PostgreSQL dumps](https://rubygems.org/pages/data) carry the full
picture if a more precise ranking is ever needed.

`--all --check` is the regression gate: a gem that compiled and no longer does
fails the build, whatever its new outcome. That gate runs in CI when a
`vendor/gems` cache is present.

## Reading the ledger

[`../conformance/gem-probe.tsv`](../conformance/gem-probe.tsv) is the record;
`gem-probe.md` is the same data rendered. Both are committed, so no absolute
path may appear in them — diagnostics are scrubbed before they are written.

The useful view is the clustering, not the total. Gaps concentrate into a few
constructs, and one fix moves every gem behind it:

```console
$ awk -F'\t' '$3=="lowering-gap" {print $4}' conformance/gem-probe.tsv \
    | sed 's/(.*//' | sort | uniq -c | sort -rn
```
