# Corpus provenance

`test/` is vendored from the `test/` suite of
[spinel](https://github.com/matz/spinel) (MIT License, Copyright (c) 2024-
Yukihiro Matsumoto), zeo's C-emitting predecessor: ~2,300 golden-output
programs (`*.rb` + `.rb.expected` stdout snapshots, `.err.expected` /
`.args` / `.stdin` sidecars, fixture subdirectories, and `analyze_fail/`
compile-rejection tests), every snapshot oracle-verified against real
`ruby`.

The directory keeps its upstream `test/` name on purpose: sidecar `.args`
files and in-test fixture reads use `test/...`-relative paths with the
working directory at this file's directory (upstream's repo root), and
keeping the name means zero edits to vendored tests. The spinel `rbs/` and
`rbs-seed/` trees (RBS extraction, out of zeo's scope) are not vendored.

This copy is zeo's living suite -- it may diverge from spinel's as tests
are added, ported (e.g. spinel's `ffi_*` intrinsic tests moving to the real
`ffi` gem API), or re-oracled. It is discovered by default by
`cargo run -p xtask -- conformance run`; `--dir` / `$SPINEL_TEST_DIR`
still override for running against an external checkout.
