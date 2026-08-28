//! `Kernel`'s run-time `require`/`require_relative` -- the dynamic half the
//! compiler could not resolve at compile time. The `ruby_module!` rows stay
//! in `mod.rs` and call these by bare name.

use super::*;

pub(crate) fn dynamic_require(arg1: &RubyValue) -> Result<RubyValue, crate::Signal> {
    let path = crate::builtins::convert::to_rstr(arg1)?
        .lock()
        .to_utf8_lossy()
        .into_owned();
    // ...unless the front end already spliced this very file, in which case
    // Ruby's own answer for an already-loaded feature -- `false` -- is both
    // correct and what the caller expects.
    if feature_already_loaded(&path) {
        return Ok(RubyValue::Bool(false));
    }
    // A unit the front end compiled in for exactly this case -- a require whose
    // target only the running program knows. See `crate::features`.
    if let Some(result) = crate::features::load_feature(&path) {
        return result.map(RubyValue::Bool);
    }
    // ...and, failing that, the file on DISK: a target under no compile-time
    // root at all (`$LOAD_PATH.unshift(dir); require "x"`, or an absolute
    // path). Compiled where it is found, by the same compiler an `eval`
    // reaches.
    if let Some(result) = crate::features::load_from_disk(&path, 0, false) {
        return result.map(RubyValue::Bool);
    }
    // `#path` carries the feature as WRITTEN. CRuby absolutizes it for
    // `require_relative` only, against the calling file's directory -- a
    // compiled binary has no such directory, so the argument stands.
    // A feature zeo DECLINES says so; everything else keeps CRuby's bare
    // wording. Shared with the compiler's loader through the ABI, the only
    // thing the two sides agree on.
    Err(missing_feature_error(&path))
}

/// The `require_relative` runtime body: CRuby resolves the path against the
/// CALLING file's directory (`rb_f_require_relative`), and so does zeo -- the
/// innermost compiled frame carries the spliced file's canonical path, and
/// the compiled-in units register under exactly that absolutized spelling.
/// That is what makes an `autoload`-DSL helper's `require_relative.call(f)`
/// land on the unit for the file `f` names (rspec-support's
/// `define_optimized_require_for_rspec` is the corpus case). An argument that
/// cannot be absolutized (already absolute, or no compiled frame below)
/// resolves exactly like `require`; a miss raises with the ABSOLUTIZED path,
/// which is the message shape CRuby's `require_relative` has.
pub(crate) fn dynamic_require_relative(arg1: &RubyValue) -> Result<RubyValue, crate::Signal> {
    let path = crate::builtins::convert::to_rstr(arg1)?
        .lock()
        .to_utf8_lossy()
        .into_owned();
    let absolutized = (!path.starts_with('/'))
        .then(crate::frames::current_location)
        .flatten()
        .and_then(|(file, _)| {
            // The MAIN file's frames carry its path AS GIVEN (`__FILE__`'s
            // rule), so a relative one resolves against the process cwd first
            // -- units register under canonical absolute spellings, and this
            // is how `zeo tests/foo.rb` finds `tests/foo/…` targets.
            let file = std::path::Path::new(file);
            let file = if file.is_absolute() {
                file.to_path_buf()
            } else {
                std::env::current_dir().ok()?.join(file)
            };
            Some(lexical_join(file.parent()?, &path))
        });
    let Some(abs) = absolutized else {
        return dynamic_require(arg1);
    };
    if feature_already_loaded(&abs) {
        return Ok(RubyValue::Bool(false));
    }
    if let Some(result) = crate::features::load_feature(&abs) {
        return result.map(RubyValue::Bool);
    }
    // ... and the same file with its symlinks resolved. The join above is
    // LEXICAL and starts from `__FILE__`, which carries the path the program
    // was invoked with, while a unit registers under the CANONICAL spelling
    // -- so a directory reached through a symlink asks under a name no unit
    // holds. On macOS every temp dir is one (`/var` is `/private/var`), and
    // the miss was silent: the target was compiled in, the require answered
    // as though it were absent, and the file's module was rebuilt at run time
    // -- so an `autoload` registered on that second module while the read
    // went to the concealed first one.
    if let Some(real) = canonical_feature(&abs) {
        if feature_already_loaded(&real) {
            return Ok(RubyValue::Bool(false));
        }
        if let Some(result) = crate::features::load_feature(&real) {
            return result.map(RubyValue::Bool);
        }
    }
    // The as-written spelling second: a unit registered under its bare
    // feature name (`require_relative "version"` next to a load-path root)
    // still resolves, matching the compiler's own root-relative fallback.
    if feature_already_loaded(&path) {
        return Ok(RubyValue::Bool(false));
    }
    if let Some(result) = crate::features::load_feature(&path) {
        return result.map(RubyValue::Bool);
    }
    // The ABSOLUTIZED spelling on disk -- `require_relative` resolves
    // against the calling file's directory, which is what the frame carries.
    if let Some(result) = crate::features::load_from_disk(&abs, 0, false) {
        return result.map(RubyValue::Bool);
    }
    Err(missing_feature_error(&abs))
}

/// A unit key spelling with its symlinks resolved, or `None` when the file is
/// not on this machine -- which is the ordinary case for a compiled-in unit,
/// and why this is only ever asked after a lookup has already missed.
///
/// The `.rb` is appended rather than set: a feature is keyed WITHOUT its
/// extension, and `Path::with_extension` would eat the tail of a name like
/// `net.http`.
fn canonical_feature(feature: &str) -> Option<String> {
    let real = std::path::Path::new(&format!("{feature}.rb"))
        .canonicalize()
        .ok()?;
    Some(real.with_extension("").to_string_lossy().into_owned())
}

/// `dir` + `rel`, normalized LEXICALLY (`.`/`..` folded without touching the
/// filesystem -- the file need not exist on the machine the binary runs on).
pub(super) fn lexical_join(dir: &std::path::Path, rel: &str) -> String {
    let mut parts: Vec<&str> = dir
        .to_str()
        .unwrap_or_default()
        .split('/')
        .filter(|c| !c.is_empty() && *c != ".")
        .collect();
    for c in rel.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            c => parts.push(c),
        }
    }
    format!("/{}", parts.join("/"))
}

/// The `LoadError` a feature that is not compiled in raises, carrying `#path`
/// and -- when the front end reached the file but could not lower it -- the
/// reason it declined. Shared by `require` and by `Module#autoload`, which
/// stands in for the same load.
pub(crate) fn missing_feature_error(path: &str) -> crate::Signal {
    let msg = match zeo_abi::declined_feature_reason(path)
        .or_else(|| crate::features::decline_reason(path))
    {
        Some(reason) => format!("cannot load such file -- {path}: {reason}"),
        None => format!("cannot load such file -- {path}"),
    };
    let sig = crate::builtins::load_error!("{}", msg);
    if let crate::signal::Signal::Raise(exc) = &sig {
        crate::builtins::exception::set_load_error_path(exc, path);
    }
    sig
}

/// Whether `path` names a file the front end already spliced -- that is,
/// whether it is in `$LOADED_FEATURES` (see `globals::seed_loaded_features`).
///
/// Compared as a suffix on a path boundary, not for equality. The seeded
/// entries are canonical absolute paths, while a dynamic require may name the
/// file relatively (`require_relative "smtp/auth_plain"`, with or without
/// `.rb`). Suffix matching finds both spellings, and the `/` boundary keeps
/// `auth_plain.rb` from matching `not_auth_plain.rb`.
///
/// A ONE-SEGMENT name needs more than that boundary, because a bare word is
/// too weak an identity: `require "fileutils"` matched bundler's
/// `.../bundler/vendor/fileutils/lib/fileutils.rb`, answered `false`, and left
/// `FileUtils` undefined for rubygems' next line. So a bare name must sit
/// directly under a `$LOAD_PATH` root -- CRuby's own rule, which resolves the
/// name against that list before it consults `$LOADED_FEATURES` at all. A
/// multi-segment name keeps the plain boundary rule: it already carries enough
/// path to identify one file.
pub(crate) fn feature_already_loaded(path: &str) -> bool {
    let RubyValue::Array(features) = crate::globals::global_get(0, "$LOADED_FEATURES") else {
        return false;
    };
    let wanted = path.trim_end_matches(".rb");
    let roots = load_path_roots(wanted);
    features.lock().iter().any(|f| {
        let RubyValue::Str(s) = f else { return false };
        let loaded = s.lock().to_utf8_lossy().into_owned();
        let loaded = loaded.trim_end_matches(".rb");
        loaded == wanted
            || loaded.strip_suffix(wanted).is_some_and(|head| {
                head.ends_with('/')
                    && match &roots {
                        None => true,
                        Some(roots) => roots.iter().any(|r| r == head.trim_end_matches('/')),
                    }
            })
    })
}

/// `$LOAD_PATH` as plain strings, or `None` when `wanted` carries a `/` and so
/// needs no root check. Read once per call rather than per loaded feature.
fn load_path_roots(wanted: &str) -> Option<Vec<String>> {
    if wanted.contains('/') {
        return None;
    }
    let RubyValue::Array(paths) = crate::globals::global_get(0, "$LOAD_PATH") else {
        return Some(Vec::new());
    };
    let paths = paths.lock();
    Some(
        paths
            .iter()
            .filter_map(|p| match p {
                RubyValue::Str(s) => Some(s.lock().to_utf8_lossy().into_owned()),
                _ => None,
            })
            .collect(),
    )
}
