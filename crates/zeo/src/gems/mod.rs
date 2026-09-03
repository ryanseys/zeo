//! Where a `require` finds its library, and what zeo says about the answer.
//!
//! Four tiers answer a feature name, and this directory holds all four plus
//! the record of which one won:
//!
//! | Module | Question |
//! |---|---|
//! | [`bundled`] | which libraries zeo SHIPS, and where each tree is |
//! | [`default_gems`] | which of those a stock ruby carries as a default gem |
//! | [`project`] | a user's own `Gemfile.lock` and gem store |
//! | [`report`] | the disclosure record: how each require was really satisfied |
//! | [`bundle`] | `zeo gem` and `zeo bundle`, the shipped binstubs |

pub mod bundle;
pub mod bundled;
pub mod default_gems;
pub mod project;
pub mod report;
