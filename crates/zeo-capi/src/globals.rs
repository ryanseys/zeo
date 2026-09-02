//! Filling `rb_cObject`, `rb_eArgError` and the other 90.
//!
//! An extension reads these directly -- `rb_define_class_under(rb_cObject,
//! ...)` is the first line of half the `Init_` functions in the census -- so
//! each is a real `VALUE` symbol rather than a stub. `cext/stubs.rs` defines
//! them, all starting as `Qundef`, and this is what moves them.
//!
//! # The naming rule, and why it is checked rather than assumed
//!
//! MRI's convention is `rb_c<Class>`, `rb_m<Module>`, `rb_e<Exception>`. So
//! `rb_cString` is `String` and `rb_eArgError` is `ArgumentError` -- except
//! that the C name is often ABBREVIATED, and the abbreviation is not
//! derivable: `rb_eArgError` is `ArgumentError`, `rb_eRuntimeError` is not
//! abbreviated at all, and `rb_cNilClass` matches exactly.
//!
//! So the rule is: strip the prefix, look the name up, and if it is not a
//! class zeo knows, look it up through [`ABBREVIATIONS`]. A global that
//! matches neither stays `Qundef` and is REPORTED by
//! [`unfilled`], which a test reads. A silently unfilled global would give an
//! extension a `Qundef` where it expected `Object`, and every later call on it
//! would be wrong in a way nothing points at.

use super::stubs::{GLOBALS, Global};
use super::value::Value;
use std::sync::atomic::Ordering;
use zeo_rt::RubyValue;

/// C names whose Ruby name is not the C name with the prefix removed.
///
/// Every one is MRI's own abbreviation. The list is short because MRI
/// abbreviates only where the full name would be unwieldy.
const ABBREVIATIONS: &[(&str, &str)] = &[
    ("rb_eArgError", "ArgumentError"),
    ("rb_eIndexError", "IndexError"),
    ("rb_eRangeError", "RangeError"),
    ("rb_eTypeError", "TypeError"),
    ("rb_eNameError", "NameError"),
    ("rb_eNoMethodError", "NoMethodError"),
    ("rb_eZeroDivError", "ZeroDivisionError"),
    ("rb_eFloatDomainError", "FloatDomainError"),
    ("rb_eSysStackError", "SystemStackError"),
    ("rb_eNotImpError", "NotImplementedError"),
    ("rb_eScriptError", "ScriptError"),
    ("rb_eRuntimeError", "RuntimeError"),
    ("rb_eFrozenError", "FrozenError"),
    ("rb_eStandardError", "StandardError"),
    ("rb_eException", "Exception"),
    ("rb_eIOError", "IOError"),
    ("rb_eEOFError", "EOFError"),
    ("rb_eLocalJumpError", "LocalJumpError"),
    ("rb_eStopIteration", "StopIteration"),
    ("rb_eKeyError", "KeyError"),
    ("rb_eEncodingError", "EncodingError"),
    ("rb_eThreadError", "ThreadError"),
    ("rb_eSecurityError", "SecurityError"),
    ("rb_eSignal", "SignalException"),
    ("rb_eInterrupt", "Interrupt"),
    ("rb_eSystemExit", "SystemExit"),
    ("rb_eNoMemError", "NoMemoryError"),
    ("rb_eLoadError", "LoadError"),
    ("rb_eSyntaxError", "SyntaxError"),
    ("rb_eSystemCallError", "SystemCallError"),
    ("rb_eRegexpError", "RegexpError"),
    ("rb_eNoMatchingPatternError", "NoMatchingPatternError"),
    ("rb_eNoMatchingPatternKeyError", "NoMatchingPatternKeyError"),
    ("rb_mComparable", "Comparable"),
    ("rb_mEnumerable", "Enumerable"),
    ("rb_mKernel", "Kernel"),
    ("rb_mMath", "Math"),
    ("rb_mProcess", "Process"),
    ("rb_mGC", "GC"),
    ("rb_mWaitReadable", "IO::WaitReadable"),
    ("rb_mWaitWritable", "IO::WaitWritable"),
    ("rb_cBasicObject", "BasicObject"),
    ("rb_cNilClass", "NilClass"),
    ("rb_cTrueClass", "TrueClass"),
    ("rb_cFalseClass", "FalseClass"),
    ("rb_cIO", "IO"),
    ("rb_cThread", "Thread"),
    ("rb_cEncoding", "Encoding"),
    ("rb_cRandom", "Random"),
    ("rb_cBinding", "Binding"),
    ("rb_cUnboundMethod", "UnboundMethod"),
    ("rb_cRefinement", "Refinement"),
    ("rb_cMatch", "MatchData"),
    ("rb_cStat", "File::Stat"),
    ("rb_eEncCompatError", "Encoding::CompatibilityError"),
    ("rb_eMathDomainError", "Math::DomainError"),
    ("rb_mErrno", "Errno"),
    ("rb_mFileTest", "FileTest"),
    ("rb_cBox", "Ruby::Box"),
];

/// The Ruby name a `VALUE` global stands for.
fn ruby_name(c_name: &str) -> Option<String> {
    if let Some((_, ruby)) = ABBREVIATIONS.iter().find(|(c, _)| *c == c_name) {
        return Some((*ruby).to_string());
    }
    // `rb_cString` -> `String`, `rb_mFileTest` -> `FileTest`.
    let rest = c_name
        .strip_prefix("rb_c")
        .or_else(|| c_name.strip_prefix("rb_m"))
        .or_else(|| c_name.strip_prefix("rb_e"))?;
    rest.starts_with(char::is_uppercase)
        .then(|| rest.to_string())
}

/// Move every global zeo can name. Answers how many were filled.
///
/// Runs once, before any `Init_`. Idempotent: a second call re-reads the same
/// classes, which is what makes it safe to run again after a later extension
/// defines something.
pub fn fill() -> usize {
    let mut filled = 0;
    // The fill hands out handles, which need a scope to be pinned in. This
    // one is never popped: a `VALUE` global outlives every call by
    // definition, and MRI's own are immortal too.
    let scope = super::scope::Scope::enter();
    for (c_name, slot) in GLOBALS {
        if slot.load(Ordering::Relaxed) != super::value::Q_UNDEF {
            filled += 1;
            continue;
        }
        let Some(ruby) = ruby_name(c_name) else {
            continue;
        };
        let Some(cls) = resolve(&ruby) else {
            continue;
        };
        let Ok(v) = super::convert::to_value(&cls) else {
            continue;
        };
        scope.keep(v);
        slot.store(v, Ordering::Relaxed);
        filled += 1;
    }
    std::mem::forget(scope);
    filled
}

/// `Thread::Mutex` and friends are nested, so the walk is per segment. A
/// segment that is not a class ends the lookup: `Foo::BAR` where `BAR` is an
/// Integer is not a global anything can hold.
fn resolve(path: &str) -> Option<RubyValue> {
    let mut owner = zeo_abi::OBJECT_CLASS;
    let mut found = None;
    for seg in path.split("::") {
        let v = zeo_rt::constants::const_get(owner.0, seg)?;
        match &v {
            RubyValue::Class(cid) => owner = *cid,
            _ => return None,
        }
        found = Some(v);
    }
    found
}

/// Every global still `Qundef`, by the name C spells.
///
/// A silently unfilled global hands an extension a `Qundef` where it expected
/// `Object`, and everything it does with that is wrong in a way nothing
/// points at. So the list is reported rather than swallowed.
pub fn unfilled() -> Vec<&'static str> {
    GLOBALS
        .iter()
        .filter(|(_, g): &&(&str, &Global)| g.load(Ordering::Relaxed) == super::value::Q_UNDEF)
        .map(|(name, _)| *name)
        .collect()
}

/// One global's current value, for a caller that has the C name.
pub fn get(name: &str) -> Option<Value> {
    super::stubs::global(name).filter(|v| *v != super::value::Q_UNDEF)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The abbreviation table is a list of claims about MRI's spelling. A
    /// duplicate or a self-mapping means one of them was added twice or
    /// added where the derivation already worked.
    #[test]
    fn the_abbreviation_table_is_sorted_by_nothing_and_free_of_duplicates() {
        let mut seen = std::collections::HashSet::new();
        for (c, ruby) in ABBREVIATIONS {
            assert!(seen.insert(*c), "{c} is in the table twice");
            assert!(
                c.starts_with("rb_c") || c.starts_with("rb_m") || c.starts_with("rb_e"),
                "{c} is not a class, module or exception global"
            );
            assert!(!ruby.is_empty());
        }
    }

    /// The derivation, not the table: `rb_cString` has to become `String`
    /// without anyone writing that down.
    #[test]
    fn a_name_derives_from_its_prefix_when_it_can() {
        assert_eq!(ruby_name("rb_cString").as_deref(), Some("String"));
        assert_eq!(ruby_name("rb_mFileTest").as_deref(), Some("FileTest"));
        assert_eq!(ruby_name("rb_cArray").as_deref(), Some("Array"));
        // The abbreviations win over the derivation.
        assert_eq!(ruby_name("rb_eArgError").as_deref(), Some("ArgumentError"));
        assert_eq!(ruby_name("rb_mErrno").as_deref(), Some("Errno"));
        assert_eq!(ruby_name("rb_cStat").as_deref(), Some("File::Stat"));
        // A lowercase tail is not a class name: `rb_argv0` is a String.
        assert_eq!(ruby_name("rb_argv0"), None);
        assert_eq!(ruby_name("rb_stdout"), None);
    }

    /// The globals `fill` cannot name, listed so the set cannot grow
    /// silently. `stubs.rs` promises `every_global_is_filled`; this is the
    /// half a unit test can answer -- every class, module and exception
    /// global derives a Ruby path, and only these do not. Each stays
    /// `Qundef` today. The IO and separator `VALUE`s (`rb_stdout`,
    /// `rb_rs`, ...) are real globals an extension may read, and filling
    /// them is open work. The rest are not `VALUE`s at all -- a registry,
    /// two type descriptors, and MRI's `ruby_version`-family C strings and
    /// digit tables -- and a `VALUE` slot is the wrong shape for them.
    /// Naming them here is what keeps a new unfillable global from hiding
    /// among them.
    #[test]
    fn every_global_derives_a_name_except_the_named_few() {
        const UNFILLABLE: &[&str] = &[
            "rb_argv0",
            "rb_default_rs",
            "rb_fs",
            "rb_memory_view_exported_object_registry",
            "rb_memory_view_exported_object_registry_data_type",
            "rb_output_fs",
            "rb_output_rs",
            "rb_ractor_local_storage_type_free",
            "rb_rs",
            "rb_stderr",
            "rb_stdin",
            "rb_stdout",
            "ruby_api_version",
            "ruby_copyright",
            "ruby_description",
            "ruby_digit36_to_number_table",
            "ruby_engine",
            "ruby_hexdigits",
            "ruby_patchlevel",
            "ruby_platform",
            "ruby_release_date",
            "ruby_version",
        ];
        let unnamed: Vec<&str> = GLOBALS
            .iter()
            .map(|(n, _)| *n)
            .filter(|n| ruby_name(n).is_none())
            .collect();
        assert_eq!(
            unnamed, UNFILLABLE,
            "the set of globals fill cannot name changed"
        );
    }

    /// Every global the table names must be a global that exists. A typo
    /// here is a row that can never fill anything.
    #[test]
    fn every_abbreviation_names_a_real_global() {
        for (c, _) in ABBREVIATIONS {
            assert!(
                GLOBALS.iter().any(|(n, _)| n == c),
                "{c} is in the abbreviation table and not in GLOBALS"
            );
        }
    }
}
