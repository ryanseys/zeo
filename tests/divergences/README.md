# Decided divergences

Every program here answers differently from ruby **on purpose**, so its
`.expected` records **zeo's own** output rather than the oracle's. Matching
ruby in these cases would make zeo worse -- an unstable sort, a `move:` that
destroys the source before it refuses, a recursive parser with no floor under
it -- or cost more than the divergence does.

The directory is the marker. There is no per-file sidecar: a program in
`tests/` matches ruby, a program in `tests/gaps/` does not match ruby yet, and
a program here will never match ruby and says why in its own header.

Each file opens with the reason and, where it helps, ruby's own answer
recorded verbatim, so the divergence stays executable evidence rather than
prose. `cargo xtask bless divergence::<stem>` re-records these from **zeo**,
which is what keeps them machine-recorded like every other golden.

Adding one is a decision, not a workaround. The bar is the project rule: a
behaviour is CRuby-identical or it fails loudly. A file lands here only when
answering differently is the better engineering, and the header has to make
that case.
