# Milestones

The umbrella entry points whose working is the point of a whole effort, one
named test case each, so a regression names itself instead of arriving as one
line inside a bigger suite. `require "rubygems"` is the headline.

| | |
|---|---|
| harness | `crates/zeo/tests/goldens.rs` |
| run it | `make test-milestones` (also in `make gate` and CI) |
| re-record | `cargo xtask bless milestone::` |

## Two tiers, the same contract the rest of the corpus carries

- **`*.rb`** runs in `Mode::Pass`. It works, and it must keep working.
- **`pending/*.rb`** runs in `Mode::Xfail`. It does not work yet, it is
  tracked, and the day it starts matching ruby the suite **fails** with a
  promote message rather than staying quietly green. Promote by moving the
  `.rb` and its `.expected` up one directory.

`pending/` is to this suite what `tests/gaps/` is to `tests/` — the corpus
is a 2×2 of cost against outcome, laid out in `tests/README.md`. Every
golden records **ruby 4.0.6's** answer.

## Why a suite of its own

Each case splices a whole library's require graph into one program: about 55
seconds of compile and roughly 820 MiB of arena, past both of the ordinary
goldens' bounds (60s, 512 MiB). The harness raises them, the nextest default
profile opts the binary out so the dev loop stays fast, and the leg passes
`-P full` to get it back. Same shape, and the same reasoning, as
`tests/gemtests/`.

## Writing one: shapes, never versions

Ruby has RubyGems loaded before the program starts, so its `require` answers
`false` where zeo's answers `true`. Both engines agree on what the API
*does*; neither agrees on that, and a golden that prints it records a
difference that means nothing. Print `Gem::VERSION.is_a?(String)`, not
`Gem::VERSION`.

For the same reason, do not print anything that reads the machine's own gem
store: a milestone must answer the same on a developer's laptop and on a bare
CI runner. Both engines resolve `Gemfile.lock` (`make deps`), so the
gems themselves agree.
