# Differential probe matrices

One Ruby program per matrix, each row printing `name<TAB>result`. Run one
through both engines and diff the rows:

```console
$ cargo xtask probe                 # every matrix here
$ cargo xtask probe arguments       # one of them
$ cargo xtask probe --all-rows      # show the agreeing rows too
```

The exit status is the number of divergent rows, so a probe can gate a
branch once its matrix is clean.

## Why a matrix and not `cargo xtask diff`

`diff` compiles one snippet per invocation, which is the right shape for
the single-divergence loop and the wrong one for hundreds of rows. A probe
pays one compile and answers the whole surface at once — and a row that
AGREES costs one line and becomes regression cover the moment someone
breaks it.

The output names only the rows that disagree. Turn a cluster into a gap
file with `cargo xtask diff -f <snippet> -n <stem>`; the probe finds
them, `diff` files them.

## The three matrices

| file | what it asks |
|---|---|
| `arguments.rb` | does a deliberately-wrong argument raise what ruby raises? Reflection, modules, procs, collections, queues, Marshal. |
| `messages.rb` | is the message text right? Errno sites naming CRuby's own C function, comparison failures naming both operands, ruby 3.4's straight quotes, regexp and encoding wording. |
| `roundtrip.rb` | does a value survive its own serializer, and does a key keep its identity? Marshal/JSON/YAML round trips, structural sharing, `hash`/`eql?` combinations, StringIO modes. |

Round-tripping earns its place by needing no reference output: a value
that does not survive its own serializer is wrong on its own terms, and
the row says so on both engines. The differential half then catches the
cases where both engines answer, differently.

## One trap

**A probe that aborts truncates its own output, and every missing row below
reads exactly like a divergence.** The runner says so out loud when an
engine exits abnormally — read that line first. The rows most likely to
abort (self-referential containers) are deliberately last in each file for
this reason.
