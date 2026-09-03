//! The machinery `builtins/` shares, none of which is a Ruby class.
//!
//! `builtins/` is one file per class, and that rule is what makes it
//! navigable: to find `Array#pack`, open `array.rs`. These eleven files break
//! the rule -- they are conversion, formatting, sorting and waiting, used by
//! many classes and owned by none -- so they sit one level down rather than
//! diluting it.

pub mod convert;
pub mod format;
pub mod formatter;
pub mod lazy;
pub mod pack;
pub mod sort;
pub mod value_subclass;
pub mod waiter;
pub mod weak;
pub mod yielder;
pub mod zeo_eval;
