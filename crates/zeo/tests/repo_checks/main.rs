//! Repo-shape checks that need no golden corpus: filesystem hygiene over
//! `tests/` and its sidecars, the projected class surface vs the ABI table,
//! and the miette diagnostic renderings. Three subjects, one binary -- each
//! was its own integration-test target once, which cost a full link apiece
//! for 14 tests.

mod builtin_shape;
mod diagnostics;
mod goldens_hygiene;
