//! In-tree, require-gated extensions -- CRuby's `ext/` model. Each module here
//! is activated by a `require "<feature>"` the compiler recognizes as a
//! built-in feature (no filesystem file), and its class/module tables plug into
//! the same `class_table`/`class_method_table` dispatch the core builtins use.
//! Unlike the retired native-package DSL, these can construct proper
//! exceptions (they live inside `spinel-rt`).

pub(crate) mod base64;
