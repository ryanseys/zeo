//! A real typed HIR instead of a text-serialized node table: zeo's
//! `zeo_parse.c` serializes Prism's C AST to a line-oriented text format
//! that `node_table.c` re-parses into a flat, dynamically-typed `SpNode`
//! arena.
//! Since `ruby-prism` hands us a real, safe, in-process `Node` tree
//! directly, we lower straight from `ruby_prism::Node` into this typed
//! arena: one step instead of two, and no string-keyed dynamic field lookup
//! anywhere.
//!
//! Identifiers (class/method/ivar/local names) are plain `String`s here, not
//! a compact interned id. Interning them is a possible later optimization,
//! not a blocker.

mod arena;
mod ffi;
mod multi;
mod node;
mod params;
mod pattern;
mod source;

pub use arena::{ConstBinding, FeatureUnit, Hir, LoadedFile, NodeFlag, NodeId, is_internal_local};
pub use ffi::{FfiCall, FfiLib, FfiStructLayout, FfiType};
pub use multi::{MultiTarget, MultiTargetGroup};
pub use node::{
    HirNode, LastMatch, RaiseCause, RegexpFlags, RescueClause, ScopeKind, StrPart, raise_cause_node,
};
pub(crate) use node::{definition_kind, split_const_path};
pub use params::{ArrayElem, KeywordParam, KwArg, Params, Visibility};
pub use pattern::{HashPatternRest, Pattern, PatternArm};
pub use source::{DataSection, FileId, SourceFile, Span, magic_frozen_string_literal};
