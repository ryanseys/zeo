# Testing Zeo against real gems

Zeo measures gem support with `gem-probe`: it fetches a gem, runs the front
end over its entry point, and records how far it got.

Only a run of the front end answers the question. A gem whose LAYOUT resolves
cleanly can still fail to compile — concurrent-ruby is pure Ruby and 100%
resolvable, and it does not compile. Do not quote a resolvability percentage
as a compatibility number.

## Probing a gem

```console
$ tools/zeo-dev gem-probe kramdown          # newest release
$ tools/zeo-dev gem-probe rake 13.3.1       # a pinned version
$ tools/zeo-dev gem-probe --names list.txt  # one name per line
$ tools/zeo-dev gem-probe --names list.txt --limit 50 --jobs 3
```

A gem named on the command line always probes. A bulk selection skips names
the ledger already carries unless `--refresh` is passed.

## How a probe runs

Four steps, and only the first touches the network:

```
resolve  name [version]     -> an exact version
fetch    the .gem           -> vendor/gems/<name>/   (cached)
probe    require "<entry>"  -> a stage and an outcome
record   the verdict        -> measurements/gem-probe.tsv
```

Probing an unpacked tree needs no network and is deterministic, so a ledger
row reproduces from its recorded version alone.

**Resolving and fetching stay on one thread.** They touch the network and
write the shared `vendor/gems` cache, and two gems routinely share a
dependency — two workers unpacking the same one into the same directory
corrupts it. Only the compile runs wide.

**How wide, and how much each compile may hold, is one decision.** The limit
is MEMORY, not cores: one gem-scale compile has been measured at 9.8 GB
resident, and twelve at once exhausted a 16 GB machine and panicked the
kernel. A fixed fraction of RAM is the sweep's budget, each job gets an equal
share, and the share is handed to the child as `ZEO_MEMORY_LIMIT`. `--jobs`
trades job count against per-job headroom rather than multiplying an unbounded
number. On a 16 GB / 12-core machine it derives 4.

**The sweep probes one binary for its whole run.** `target/probe-bin/zeo-<sha>`
is a snapshot, so a dev build beside the sweep cannot change what it is
measuring halfway through. Run a side-by-side sweep at `--jobs 3`; the spare
slot is the headroom that keeps concurrent probe and dev rustc off each other.

**Each compile is a subprocess**, because a sweep must survive a gem that
kills the compiler. `catch_unwind` catches an unwinding panic; it cannot catch
an abort, a stack overflow, or a compile that never finishes. The OS ends all
four the same way, and `--timeout` (default 600 s) bounds the last.

### The entry point is a file that exists

Guessing from the name alone is not good enough, and failing quietly is the
reason: `require "activerecord"` names no file, Zeo lowers an unresolvable
require to a RUNTIME `Kernel#require` rather than failing, and codegen then
trivially succeeds having compiled none of the gem. Every Rails gem reported
`compiles` that way once.

So there is a four-tier ladder, and every tier names a file that exists: the
gem's own name against its load path; then every top-level file the roots
ship; then the roots of the gem's own require graph (for the pre-convention
`lib/<dir>/` layout); then its ruby-shebang executables, `load`ed by absolute
path exactly as a RubyGems binstub runs them.

### Dependencies, and an isolated view

The subject is probed against its full runtime dependency **closure**, walked
breadth-first and capped at depth 6 and 200 gems. A one-level list was the
probe's largest source of false verdicts: nanoc's view carried nanoc-core but
not nanoc-core's own `ddplugin`, and 16 gems recorded a lowering-gap for a
program CRuby would have stopped with a `LoadError`.

Those gems are linked into a view holding only the subject and its
dependencies. Pointing the probe at the whole of `vendor/gems` made a verdict
depend on which other gems happened to be cached — kramdown reported one gap
alone and a different one once kramdown-parser-gfm sat beside it. The subject
is the distinguished root (`--root-gem`), so a feature it provides resolves to
it and never to an alphabetically earlier dependency squatting the path.

## Reading a verdict

**Two columns carry it, and they are read together.** `stage` is the RUNG the
row is about; `outcome` is what happened there. Neither claims anything alone:

```
excon      1.2.5   codegen   ok             -- CLIF was emitted
Authorizr  0.2.1   codegen   lowering-gap   -- it was not, and this is why
```

The ladder is:

```
fetch → unpack → parse → lower → analyze → codegen
```

The four middle rungs are Zeo's own front-end passes, and a rejection is
recorded at the pass that **made** it — Zeo prints that as the diagnostic's
code (`zeo::parse`, `zeo::lower`, `zeo::analyze`, `zeo::codegen`). A lowering
gap is a construct the front end will not translate; an analyze rejection is a
definition it will not register; a codegen rejection is a position it will not
emit into. Those are different kinds of work.

| Outcome | Meaning |
|---|---|
| `ok` | The front end accepted the gem's entry point. |
| `lowering-gap` | Zeo cannot lower a construct. The detail names it. |
| `compiler-panic` | Zeo panicked. A bug, not a stated limit. |
| `timeout` | Still running at `--timeout`. No verdict at all. |
| `out-of-memory` | Hit the ceiling this sweep handed it. A fact about the host. |
| `invalid-ruby` | Not Ruby any release parses. Zeo parses with prism, CRuby's own parser. |
| `native-extension` | Names its compiled half by path, so no gem's `extensions` says what to build. See [EXTENSIONS.md](EXTENSIONS.md). |
| `precompiled-extension` | Ships a `.so` built against CRuby's ABI. Install the ruby-platform variant and zeo compiles it from source. |
| `extension-build-failed` | zeo tried to build the gem's C and the compiler or linker refused; the detail names the gem. |
| `missing-dependency` | A `require` reached outside the gem and its closure. |
| `ambiguous-require` | Two gems on the view provide it. A fact about the probe, not the gem. |
| `no-lib-dir` | A load path that does not exist in the archive. |
| `no-entry-point` | Roots, but no file any tier could name. |
| `meta-gem` | No Ruby at all — a gemspec that names dependencies, like `rails`. |
| `ext-only` | The gem's code IS its C extension. |
| `platform-gem` | Released only as prebuilt platform artifacts; no `ruby` gem exists. |
| `fetch-failed` | No such gem, or the registry could not be reached. |

A panic, a timeout and a memory kill are each their own outcome rather than
folded into `lowering-gap`: a gap is a limit Zeo REPORTED, a panic is a bug it
did not, a timeout is the absence of a verdict, and an out-of-memory says what
this machine could hold rather than anything about the gem.

The `where` column is the site Zeo named, `<repo-relative path>:<line>`. The
detail alone says what Zeo refused, not where — `subclassing the built-in type
Module` names a construct that appears in dozens of files across a dependency
tree.

## What `ok` does and does not mean

It means the gem's entry point reached code generation. It does **not** mean
the gem works.

- Nothing is linked and nothing is run.
- The entry point is whatever the gem calls its own. Some gems make that the
  whole library; `actionpack`'s `action_pack.rb` is one line that requires a
  version file. `ok` is a weak signal for the latter.
- Zeo can decline a unit and defer it to a runtime `LoadError`, so the gem's
  own code may not have been compiled at all.
- A gem that compiles can still fail at run time on behaviour Zeo diverges on.

Read an `ok` row as "the compiler has no objection to this source", and
nothing more.

## The ledger is output, not source

`measurements/` is not in git. The ledger is 21 MB and 195,778 rows of
measurement, which is not an input to any build or test. A sweep writes it
locally; a refreshed one is attached to a GitHub release by hand.

That is also why no CI step gates on it: with neither a committed corpus nor a
committed ledger, there is nothing for a regression check to compare against.
A sweep is a deliberate act, run when someone wants the number.

```console
# how many gems compile, in a ledger you have
$ awk -F'\t' 'NR>1 && $4=="ok"' measurements/gem-probe.tsv | wc -l

# the most common gap, by message
$ awk -F'\t' '$4=="lowering-gap" {print $7}' measurements/gem-probe.tsv \
    | sort | uniq -c | sort -rn | head -20
```

Two traps worth knowing when reading a ledger:

- **A contiguous ALPHABETICAL band of one diagnostic is a truncated sweep, not
  a cluster.** Roughly 240 rows were once read as a real signal that way.
- **A big cluster can be one message masking a different upstream refusal.**
  Grep `unit declined` under `--log-level warn` first.
