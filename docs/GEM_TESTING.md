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
$ cargo xtask gem-probe --popular 1000    # the 1000 most downloaded gems
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
record    the Outcome         -> conformance/gem-probe-*.tsv      (committed)
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

The corpus is the curated, reviewable list. Two selectors go wider, and both
accumulate into the same ledger:

- `--popular N` walks the download ranking, most-downloaded first.
- `--index` walks every gem the registry knows — useful for finding constructs
  nothing popular exercises, but it is **alphabetical**, so it meets a
  well-known gem only by chance.

Prefer `--popular` for coverage that means something. The registry has no top-N
endpoint (`/api/v1/downloads/top.json` is gone) and
[rubygems.org/stats](https://rubygems.org/stats) stops at 100, so the ranking
comes from the search API, which answers a `*` query sorted by download count.
The cache at `conformance/rubygems-popular.txt` is a prefix of that ranking, so
raising N extends it rather than re-fetching. The
[weekly PostgreSQL dumps](https://rubygems.org/pages/data) carry the full
picture if a more precise ranking is ever needed.

`--all --check` is the regression gate: a gem that compiled and no longer does
fails the build, whatever its new outcome. That gate runs in CI when a
`vendor/gems` cache is present.

## Provenance: the ledger holds verdicts, git holds everything else

The ledger records a gem, a version, an outcome and a detail. It carries no
timestamp, no build SHA and no Zeo version — deliberately.

It is a committed file, so git already holds all of that, and holds it more
reliably than anything written into the rows:

```console
# when did any verdict last change, and in which commit?
$ git log --oneline -- conformance/gem-probe-*.tsv

# when did ONE gem change, and to what?
$ git log -p -S kramdown -- conformance/gem-probe-*.tsv

# what did a release improve?
$ git diff v0.1.0..v0.2.0 -- conformance/gem-probe-*.tsv
```

Writing the time and build into the file would duplicate that, and duplicating
it costs more than it gives: a timestamp column rewrites every row on every
run, so a 700-line diff appears when nothing changed and the real signal is
buried. Leaving it out makes the ledger a pure function of the corpus and the
Zeo build — two runs on one build produce identical bytes, so **any diff is a
real change**.

The one thing this does not cover is a probe you never commit. Commit the
ledger; that is what makes the result a record rather than a note.

## Reading the ledger

The ledger is two files, one per verdict:

| File | Holds |
|---|---|
| [`../conformance/gem-probe-compiles.tsv`](../conformance/gem-probe-compiles.tsv) | the gems Zeo compiles |
| [`../conformance/gem-probe-fails.tsv`](../conformance/gem-probe-fails.tsv) | the gems it does not, and why |

Both carry the same four columns, so a gem keeps its shape when a fix moves it
across. A gem is in one file or the other, never both — `gem-probe` refuses to
run if it finds one twice, because the two copies disagree about the verdict
and there is no safe way to pick. `gem-probe.md` renders both.

All three are committed, so no absolute path may appear in them — diagnostics
are scrubbed before they are written.

The registry name index is cached at `conformance/rubygems-names.txt` and is
gitignored: ~3MB of upstream data that changes daily, and the repository
refuses tracked files that size. `--refresh-index` takes a newer copy.

**A count of gems is not a count of code.** The corpus is the dependency
closure of the most downloaded gems, and that pulls in the whole `aws-sdk-*`
family — well over half the rows. Those are machine-generated from one
template, so they share one construct and fail identically: almost every one of
them on `define_method`'s second argument. Read the corpus both ways.

Excluding `aws-*` is the better signal for the language. Including them is the
better signal for "will my Gemfile work", since a dependency that fails 400
times still fails.

```console
# the split, both ways
$ for f in conformance/gem-probe-*.tsv; do
    echo "$f: $(grep -vc '^#' $f) total, $(grep -vc '^#\|^aws' $f) excluding aws-*"
  done
```

The useful view is the clustering, not the total. Gaps concentrate into a few
constructs, and one fix moves every gem behind it:

```console
$ awk -F'\t' '$3=="lowering-gap" {print $4}' conformance/gem-probe-fails.tsv \
    | sed 's/(.*//' | sort | uniq -c | sort -rn
```
