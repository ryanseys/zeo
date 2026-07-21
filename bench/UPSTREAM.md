# Benchmark provenance

Copied from the `benchmark/` suite of [spinel](https://github.com/matz/spinel)
(MIT License, Copyright (c) 2024- Yukihiro Matsumoto), zeo's C-emitting
predecessor, whose golden-output benchmark discipline this suite continues.
Many programs are in turn adaptations of classic Ruby benchmark corpora
(the Computer Language Benchmarks Game shapes, yjit-bench-style micro
benchmarks).

Each `bm_<name>.rb` prints deterministic output; `bm_<name>.rb.expected` is
that output, oracle-verified against real `ruby`. `cargo run -p xtask -- bench`
compiles each with `zeo -o` (release runtime), verifies the output matches
(a wrong answer fails the run -- speed of a wrong answer is meaningless),
times the compiled binary, and compares against `baseline.tsv`.

Regenerate the baseline after an intentional performance change with
`cargo run -p xtask -- bench --update-baseline` and commit the diff --
the baseline is the reviewable record of every accepted regression or win.
