//! Every variable family that is not an ivar, following CRuby's
//! `variable.c`: globals, constants, class variables and the two the
//! compiler owns -- the last match and each flip-flop's latch.
//!
//! Instance variables live with the object that holds them
//! (`value::ivars`), which is why they are not here.

pub(crate) mod civars;
pub mod constants;
pub(crate) mod cvars;
pub(crate) mod flipflop;
pub mod globals;
pub(crate) mod lastmatch;
