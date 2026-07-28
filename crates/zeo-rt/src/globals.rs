//! `$foo`-family global variables -- keyed `(box_id, name)`:
//! every `Ruby::Box` gets a fully SEPARATE global table with no fallback
//! layer at all, which is empirically faithful to CRuby's box model (a box
//! reads `nil` for a `$g` main set: its clone-on-first-read pulls from the
//! ROOT entry, which user code never writes -- variable.c:1050, verified in
//! the plan's research contract). Box 0 is the root/main program. Same
//! `LazyLock<Mutex<_>>` pattern as `cvars`/`constants`/the Symbol interner.

use crate::RubyValue;
use parking_lot::Mutex;
use crate::FMap;
use std::sync::LazyLock;

static GLOBALS: LazyLock<Mutex<FMap<(u32, String), RubyValue>>> =
    LazyLock::new(|| Mutex::new(FMap::default()));

/// `alias $new $old` -- alias name -> the name whose STORAGE it shares.
///
/// A real alias, not a copy: the two names are one slot, and writing
/// EITHER is visible through the other (oracle-verified in both
/// directions -- `$orig = 9` makes `$copy` read 9, and `$copy = 7` makes
/// `$orig` read 7). So this resolves on every access rather than copying a
/// value at alias time.
///
/// Keyed `(box_id, name)` like `GLOBALS` itself, since an alias is
/// per-box state exactly as the variable is.
static ALIASES: LazyLock<Mutex<FMap<(u32, String), String>>> =
    LazyLock::new(|| Mutex::new(FMap::default()));

/// The name whose storage `name` actually refers to -- itself, unless it
/// was aliased. Chains are followed (`alias $b $a; alias $c $b` makes all
/// three one slot), with a depth cap: real Ruby resolves the target AT
/// ALIAS TIME, so a cycle can't arise from Ruby source, but a cap beats
/// hanging if one ever did.
fn resolve(box_id: u32, name: &str) -> String {
    let aliases = ALIASES.lock();
    let mut cur = name.to_string();
    for _ in 0..16 {
        match aliases.get(&(box_id, cur.clone())) {
            Some(target) => cur = target.clone(),
            None => return cur,
        }
    }
    cur
}

/// `alias $new $old` -- makes `$new` name `$old`'s storage. The target is
/// resolved through any existing alias first, so every name in a chain
/// points at the one real slot.
pub fn global_alias(box_id: u32, new_name: &str, old_name: &str) {
    let target = resolve(box_id, old_name);
    ALIASES
        .lock()
        .insert((box_id, new_name.to_string()), target);
}

/// A global whose value lives somewhere other than the `$foo` table: the
/// exception being handled, the last child's status, the last match and the
/// pieces derived from it.
///
/// Codegen reads these directly at their own spellings, which is faster and
/// is where `$1`/`$&` already went. This registry is what makes an ALIAS of
/// one read the same place -- `require "English"` names every one of them,
/// and an alias resolves to a NAME, which then lands here.
enum Special {
    /// `$!`
    ErrorInfo,
    /// `$@` -- the backtrace of `$!`, not a slot of its own.
    ErrorPosition,
    /// `$?`
    ChildStatus,
    /// `$~`
    MatchData,
    /// `$&` is group 0; `$1`..`$n` are the rest.
    Group(usize),
    /// `` $` ``
    PreMatch,
    /// `$'`
    PostMatch,
    /// `$+`
    LastGroup,
}

fn special_of(name: &str) -> Option<Special> {
    Some(match name {
        "$!" => Special::ErrorInfo,
        "$@" => Special::ErrorPosition,
        "$?" => Special::ChildStatus,
        "$~" => Special::MatchData,
        "$&" => Special::Group(0),
        "$`" => Special::PreMatch,
        "$'" => Special::PostMatch,
        "$+" => Special::LastGroup,
        // `$1`.. -- and NOT `$0`, which is the program name.
        _ => match name.strip_prefix('$')?.parse::<usize>() {
            Ok(n) if n > 0 => Special::Group(n),
            _ => return None,
        },
    })
}

fn special_get(special: Special) -> RubyValue {
    match special {
        Special::ErrorInfo => crate::current_exception().unwrap_or(RubyValue::Nil),
        Special::ErrorPosition => match crate::current_exception() {
            Some(exc) => crate::dispatch::send_value(&exc, crate::symbol::wk::backtrace(), &[], None)
                .unwrap_or(RubyValue::Nil),
            None => RubyValue::Nil,
        },
        Special::ChildStatus => crate::last_child_status(),
        Special::MatchData => crate::last_match(),
        Special::Group(n) => crate::last_match_group(n),
        Special::PreMatch => crate::last_match_pre(),
        Special::PostMatch => crate::last_match_post(),
        Special::LastGroup => crate::last_match_last_group(),
    }
}

/// `nil` for a `$foo` never yet written IN THIS BOX -- matches real Ruby's
/// own behavior for reading a global before any assignment ran (no
/// `NameError`, unlike an unset constant -- see `constants::const_get`'s
/// docs for that distinction).
pub fn global_get(box_id: u32, name: &str) -> RubyValue {
    let target = resolve(box_id, name);
    if let Some(special) = special_of(&target) {
        return special_get(special);
    }
    GLOBALS
        .lock()
        .get(&(box_id, target))
        .cloned()
        .unwrap_or(RubyValue::Nil)
}

/// The plain store, for the runtime's own seeding (`$stdout`, `$0`, ...),
/// which never targets a [`Special`]. Ruby-level assignment goes through
/// [`global_assign`].
pub fn global_set(box_id: u32, name: &str, value: RubyValue) {
    GLOBALS
        .lock()
        .insert((box_id, resolve(box_id, name)), value);
}

/// `$g = value` as written in Ruby. Assigning a read-only special is a
/// `NameError` naming the spelling the program used, so an alias reports
/// itself (`$MATCH is a read-only variable`) rather than its target. At their
/// own spellings these are SyntaxErrors that prism rejects before lowering,
/// which is why only the aliased path can reach here.
pub fn global_assign(box_id: u32, name: &str, value: RubyValue) -> Result<(), crate::Signal> {
    let target = resolve(box_id, name);
    match special_of(&target) {
        None => {
            GLOBALS.lock().insert((box_id, target), value);
            Ok(())
        }
        Some(Special::MatchData) => {
            crate::set_last_match(match value {
                RubyValue::MatchData(m) => Some(m),
                _ => None,
            });
            Ok(())
        }
        Some(Special::ErrorPosition) => match crate::current_exception() {
            Some(exc) => crate::builtins::exception::apply_custom_backtrace(&exc, &value),
            None => Err(crate::raise_error("ArgumentError", "$! not set".to_string())),
        },
        Some(_) => Err(crate::raise_error(
            "NameError",
            format!("{name} is a read-only variable"),
        )),
    }
}

/// The globals that CRuby gives a meaningful default: `$/` (the input record
/// separator) starts as `"\n"`, and `$0`/`$PROGRAM_NAME` (one aliased slot,
/// writing either changes both -- CRuby's rule) start as the program path.
/// For a compiled binary that is the executable itself -- the honest
/// analogue of CRuby's script path (corpus assertions are portable:
/// `.length > 0`-style, never the exact text). The other punctuation
/// globals (`$;`, `$,`, ...) start nil, which the unset read already
/// answers. Called once at bootstrap.
pub fn seed_default_globals() {
    global_set(0, "$/", RubyValue::Str(crate::string_new("\n".to_string())));
    let prog = std::env::args().next().unwrap_or_default();
    global_set(0, "$0", RubyValue::Str(crate::string_new(prog)));
    global_alias(0, "$PROGRAM_NAME", "$0");
    global_set(0, "$$", RubyValue::Int(i64::from(std::process::id())));

    // Real Arrays, but empty, and they stay that way: every `require` in a
    // compiled program was resolved at COMPILE time, so there is no runtime
    // search path for a push to affect and no feature list to append to.
    // Seeding them as Arrays is what lets the near-universal
    // `$LOAD_PATH.unshift File.dirname(__FILE__)` preamble run instead of
    // raising on nil.
    global_set(0, "$LOAD_PATH", RubyValue::Array(crate::array_new(Vec::new())));
    global_alias(0, "$:", "$LOAD_PATH");
    global_alias(0, "$-I", "$LOAD_PATH");
    global_set(
        0,
        "$LOADED_FEATURES",
        RubyValue::Array(crate::array_new(Vec::new())),
    );
    global_alias(0, "$\"", "$LOADED_FEATURES");
}

/// Fills `$LOADED_FEATURES` with the files the front end spliced -- called once
/// from generated `main()`, after `seed_default_globals`.
///
/// It stays a real, mutable Array (a program may push to it), but nothing else
/// ever appends: every require was resolved at compile time. Its one functional
/// use is [`crate::builtins::kernel::feature_already_loaded`], which lets a
/// DYNAMIC require of an already-spliced path answer `false` instead of raising.
pub fn seed_loaded_features(paths: &[&str]) {
    let values = paths
        .iter()
        .map(|p| RubyValue::Str(crate::string_new((*p).to_string())))
        .collect();
    global_set(0, "$LOADED_FEATURES", RubyValue::Array(crate::array_new(values)));
}

/// Whether `name` has ever been assigned in this box -- backs
/// `defined?($g)`, which answers `"global-variable"` only for an assigned
/// user global and `nil` for one that was never written (unlike `global_get`,
/// which reads any unset global as `nil`). Predefined special globals
/// (`$!`, `$~`, ...) are handled by the caller and never reach here at their
/// own spellings.
///
/// An ALIAS is defined from the moment it is created, whatever its target
/// currently reads: `defined?($MATCH)` is `"global-variable"` before any
/// match has run, where `defined?($&)` is nil. Oracle-verified.
pub fn global_defined(box_id: u32, name: &str) -> bool {
    if ALIASES.lock().contains_key(&(box_id, name.to_string())) {
        return true;
    }
    GLOBALS
        .lock()
        .contains_key(&(box_id, resolve(box_id, name)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int_of(v: RubyValue) -> Option<i64> {
        match v {
            RubyValue::Int(i) => Some(i),
            _ => None,
        }
    }

    /// The property the alias table exists for, and the one a
    /// copy-at-alias-time implementation would fail: ONE slot, two names,
    /// and a write through EITHER is visible through the other.
    /// Oracle-verified in both directions.
    #[test]
    fn an_alias_shares_storage_bidirectionally() {
        global_set(0, "$g_orig", RubyValue::Int(5));
        global_alias(0, "$g_copy", "$g_orig");
        assert_eq!(int_of(global_get(0, "$g_copy")), Some(5));
        // write through the ALIAS -> visible via the original
        global_set(0, "$g_copy", RubyValue::Int(7));
        assert_eq!(int_of(global_get(0, "$g_orig")), Some(7));
        // ...and write through the ORIGINAL -> visible via the alias
        global_set(0, "$g_orig", RubyValue::Int(9));
        assert_eq!(int_of(global_get(0, "$g_copy")), Some(9));
    }

    /// Aliasing a never-assigned global is legal; both names then read nil,
    /// and a later write through the target shows up.
    #[test]
    fn aliasing_an_unset_global_is_legal() {
        global_alias(0, "$g_b", "$g_never_set");
        assert!(matches!(global_get(0, "$g_b"), RubyValue::Nil));
        global_set(0, "$g_never_set", RubyValue::Int(1));
        assert_eq!(int_of(global_get(0, "$g_b")), Some(1));
    }

    /// A chain collapses to the one real slot: `alias $c $b` where `$b` is
    /// already an alias of `$a` makes all three the same storage. The
    /// target is resolved AT ALIAS TIME, mirroring Ruby.
    #[test]
    fn alias_chains_collapse_to_one_slot() {
        global_set(0, "$g_a", RubyValue::Int(1));
        global_alias(0, "$g_b2", "$g_a");
        global_alias(0, "$g_c2", "$g_b2");
        global_set(0, "$g_c2", RubyValue::Int(42));
        assert_eq!(int_of(global_get(0, "$g_a")), Some(42));
        assert_eq!(int_of(global_get(0, "$g_b2")), Some(42));
    }

    /// Aliases are per-box, exactly like the table they indirect into --
    /// box 1 aliasing a name must not touch box 0's reading of it.
    #[test]
    fn aliases_are_per_box() {
        global_set(0, "$g_box", RubyValue::Int(1));
        global_set(1, "$g_box", RubyValue::Int(2));
        global_alias(1, "$g_box_alias", "$g_box");
        assert_eq!(int_of(global_get(1, "$g_box_alias")), Some(2));
        // Box 0 never saw the alias at all.
        assert!(matches!(global_get(0, "$g_box_alias"), RubyValue::Nil));
    }
}
