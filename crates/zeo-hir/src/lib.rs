//! `zeo-hir`: zeo's front end as a standalone crate -- the typed HIR arena
//! (`hir`) plus the prism-tree -> HIR lowering (`lower`), extracted from the
//! compiler so the runtime's eval VM can lower source at RUN time without
//! linking the whole compiler.
//!
//! What deliberately does NOT live here: `require` resolution, gems, and
//! search paths -- those are the compiler's loader (`zeo::parse`), which
//! DRIVES lowering file-by-file and feeds it per-file state through
//! `lower::context`. This crate never touches the filesystem.

pub mod constpath;
pub mod hir;
pub mod lower;
pub mod lower_error;
pub mod rename;
