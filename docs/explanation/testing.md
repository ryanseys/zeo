# Testing

The suite asks one question about a Ruby program: does what it printed match
what is recorded in its own file. Everything below follows from keeping it to
that one question.

## Ruby is the oracle

A trailer is recorded by real ruby, at the version `.ruby-version` pins.
Nobody writes an expected answer by hand, so a test cannot encode a mistaken
belief about what Ruby does. When zeo and the recording disagree, one of them
is wrong about Ruby, and reading the diff says which.

The pin matters more than it looks. A trailer recorded by a different ruby
records *that ruby's* differences as zeo's bugs, so `bless` checks the
version and refuses rather than recording. And the oracle resolves the same
`Gemfile.lock` zeo ships, so neither side can answer a `require` with a
version the other does not have.

## One compile per program

The corpus is compiled and run once, on the JIT. It used to be run six times
— separate passes for the AOT tier, a memory ledger, a cycle census, and
three differential roads — which cost 25,000 cases and about 124 GB of disk
per run to ask questions that mostly folded into the first pass.

What folded in: the ownership ledger and the cycle census are **environment
variables on the same process**, not a different execution. They ride on the
ordinary run for almost nothing, and the census line is split out of stderr
before the comparison.

What did not fold in is `test/aot/`: a real link is a different question —
the link line, the whole-archive spelling, what a shipped binary has to
carry — and a curated set of programs asks it.

What was deleted outright: a differential road comparing zeo against itself
with a debug switch nobody ships. Comparing against *ruby's* answers is
strictly better signal, and it is what every other case already does.

## Nothing on disk

Every corpus child compiles in memory with the program cache off. Adding
tests costs time, not space. That is a deliberate constraint: a suite that
grows a gigabyte per run is a suite people stop running.

## No second verdict

There is no XFAIL mechanism in the corpus. A program either matches its
recording or the suite is red.

Programs zeo does not get right yet live in `todo/`, with ruby's answer
recorded, and **nothing runs them**. The trade is real and written down: one
of them can start working without anyone noticing. The alternative — a
harness that knows some programs must fail — buys that notification with a
second verdict on every case, and it is not worth it.

Programs that differ **on purpose** are different again. Those record zeo's
own answer under `test/divergences/`, each saying why in its header, so the
difference is a passing test rather than an exception to one.

## What the checks suite is for

`checks::` holds what a Ruby program cannot ask: that the CLIF snapshots are
unchanged, that no tracked file is over a megabyte, that no C reached the
tree, that every documented environment variable has a reader, that the
corpus's own rules hold. They run in process and cost milliseconds.
