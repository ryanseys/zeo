# Benchmark provenance

Copied from the `benchmark/` suite of [spinel](https://github.com/matz/spinel)
(MIT License, Copyright (c) 2024- Yukihiro Matsumoto), zeo's C-emitting
predecessor, whose golden-output benchmark discipline this suite continues.
Many programs are in turn adaptations of classic Ruby benchmark corpora
(the Computer Language Benchmarks Game shapes, yjit-bench-style micro
benchmarks).

Local workload divergence: `bm_tree_walker.rb`'s round count was reduced
400 -> 100 (2026-08-25) so a 10-sample criterion bank stays affordable
while zeo's dispatch-bound gap on it is still open; the polymorphic
structure is unchanged.

Each `bm_<name>.rb` prints deterministic output; `bm_<name>.rb.expected` is
that output, oracle-verified against real `ruby`. The criterion harness
(`crates/zeo/benches/programs.rs`, `make bench`) compiles each with
`zeo -o` (release runtime), verifies the output matches (a wrong answer
fails the run -- speed of a wrong answer is meaningless), and times the
compiled binary; comparisons run through criterion's saved baselines (see
`README.md` here).
