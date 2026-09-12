//! The compiled-in load path: every file the front end could not splice at a
//! fixed position, emitted as a callable unit and registered here under the
//! feature name a `require` would spell.
//!
//! Whole-program AOT resolves an ordinary `require "json"` at compile time and
//! splices it, so it never reaches this table. What does reach it is a require
//! whose target is only known when the program RUNS -- a computed string, or
//! the one every `autoload`-DSL gem builds (`ActiveSupport::Autoload#autoload`
//! joins the module name to the constant and calls `super const_name, path`).
//! Those are ordinary Ruby, and they work here for the ordinary reason: the
//! file is compiled in, and its top-level statements are a function this table
//! can call.
//!
//! What a unit does NOT change is registration. A class defined in a unit is in
//! the dispatch tables from startup, exactly like a spliced one -- zeo has
//! always separated "which classes exist" (compile time) from "when their
//! bodies run" (document position). Running a unit runs its bodies.
//!
//! Loading is once-only and re-entrant-safe, mirroring CRuby's loading table: a
//! feature already loaded answers `false`, one currently loading answers
//! `false` too (that is what makes a require cycle terminate rather than
//! recurse), and a fresh one runs its unit and answers `true`.

use crate::RubyValue;
use crate::signal::Signal;
use std::collections::{HashMap, HashSet};

/// A compiled unit: one file's top-level statements.
pub type UnitFn = fn() -> Result<RubyValue, Signal>;

/// The same unit through the C ABI: a compiled unit function from a
/// Cranelift-emitted program (`zeo_abi::abi::UnitFn`).
pub type CUnitFn = unsafe extern "C" fn(out: *mut RubyValue) -> i32;

/// Which emitter produced a unit. The two backends hand the runtime the
/// same thing in the two shapes their calling conventions allow.
#[derive(Clone, Copy)]
pub enum UnitImpl {
    Rust(UnitFn),
    C(CUnitFn),
}

impl UnitImpl {
    /// The identity a load cycle is detected by -- the function itself.
    fn identity(self) -> usize {
        match self {
            UnitImpl::Rust(f) => f as usize,
            UnitImpl::C(f) => f as usize,
        }
    }

    fn call(self) -> Result<RubyValue, Signal> {
        match self {
            UnitImpl::Rust(f) => f(),
            UnitImpl::C(f) => {
                let mut out = std::mem::MaybeUninit::<RubyValue>::uninit();
                match unsafe { f(out.as_mut_ptr()) } {
                    0 => Ok(unsafe { out.assume_init() }),
                    _ => Err(crate::signal::take_pending()
                        .expect("a status of 1 leaves a pending signal")),
                }
            }
        }
    }
}

/// The seam a file the compiler never saw reaches the compiler through --
/// the twin of [`crate::eval::EvalCompiler`], installed by the same
/// `zeo_eval_install`, so a program that can reach a run-time load carries
/// exactly the machinery a program that can `eval` does.
///
/// The compiler-side half compiles the source as a TOP-LEVEL file: its own
/// `def`s land on `Object`, its classes mint, and its own `require`s come
/// straight back here.
pub trait UnitCompiler: Send + Sync {
    fn load(&self, source: &str, path: &str, box_id: u32) -> Result<RubyValue, Signal>;
}

static UNIT_COMPILER: std::sync::OnceLock<&'static dyn UnitCompiler> = std::sync::OnceLock::new();

/// See [`UnitCompiler`].
pub fn install_unit_compiler(compiler: &'static dyn UnitCompiler) {
    let _ = UNIT_COMPILER.set(compiler);
}

/// `$LOAD_PATH.resolve_feature_path(feature)` -- where a `require` of
/// `feature` WOULD land, without loading it.
///
/// CRuby answers `[:rb, <absolute path>]` for a Ruby file, `[:so, <path>]`
/// for an extension, and `nil` when nothing supplies the name. The search
/// order is `search_required`'s: every load-path root first, the
/// statically-linked-extension table only after that.
///
/// zeo is a ruby built `--with-static-linked-ext`, so a compiled-in feature
/// has no file to name and answers `[:so, feature]` -- CRuby's own answer for
/// a static ext. A feature with a Ruby half on disk still answers `:rb`,
/// which is why the disk search runs first here too.
///
/// Box 0: `Kernel#require` resolves against box 0 as well (`kernel/load.rs`),
/// so the question and the load agree.
fn resolve_feature_path(feature: &str) -> Option<(&'static str, RubyValue)> {
    if let Some(path) = resolve_on_disk(feature, 0, true) {
        let path = path.to_string_lossy().into_owned();
        return Some(("rb", RubyValue::Str(crate::string_new(path))));
    }
    // A compiled extension on a load-path root is named as itself, `:so`.
    if let Some((path, _)) = resolve_native_on_disk(feature, 0) {
        let path = path.to_string_lossy().into_owned();
        return Some(("so", RubyValue::Str(crate::string_new(path))));
    }
    let bare = feature
        .strip_suffix(".so")
        .or_else(|| feature.strip_suffix(".bundle"))
        .or_else(|| feature.strip_suffix(".rb"))
        .unwrap_or(feature);
    // Ruby has no file for these at all, so it answers nil and so does zeo.
    if zeo_abi::CORE_WITH_NO_FILE.contains(&zeo_abi::canonical_ext_feature(bare)) {
        return None;
    }
    (zeo_abi::is_builtin_feature(bare) && build_carries_ext(bare))
        .then(|| ("so", RubyValue::Str(crate::string_new(bare.to_string()))))
}

/// Whether THIS build compiled the native half behind a gated ext feature --
/// the dual-build switch's ground truth, asked where the cargo features
/// actually live. The zeo compiler links this same build and forwards its
/// own `ext-*` features here, so its loader and this runtime's
/// `Kernel#require` agree by construction. A feature the match does not
/// name is not ext-gated and is always carried.
pub fn build_carries_ext(feature: &str) -> bool {
    match zeo_abi::canonical_ext_feature(feature) {
        "base64" => cfg!(feature = "ext-base64"),
        "bigdecimal" => cfg!(feature = "ext-bigdecimal"),
        "cgi/escape" => cfg!(feature = "ext-cgi"),
        "coverage" => cfg!(feature = "ext-coverage"),
        "date" => cfg!(feature = "ext-date"),
        "digest" | "digest/md5" | "digest/sha1" | "digest/sha2" => cfg!(feature = "ext-digest"),
        "etc" => cfg!(feature = "ext-etc"),
        "fcntl" => cfg!(feature = "ext-fcntl"),
        "ffi" => cfg!(feature = "ext-ffi"),
        "json" => cfg!(feature = "ext-json"),
        "monitor" => cfg!(feature = "ext-monitor"),
        "nkf" => cfg!(feature = "ext-nkf"),
        "openssl" => cfg!(feature = "ext-openssl"),
        "prism" => cfg!(feature = "ext-prism"),
        "psych" => cfg!(feature = "ext-psych"),
        "pty" => cfg!(feature = "ext-pty"),
        "socket" => cfg!(feature = "ext-socket"),
        "stringio" => cfg!(feature = "ext-stringio"),
        "strscan" => cfg!(feature = "ext-strscan"),
        "syslog" => cfg!(feature = "ext-syslog"),
        "zlib" => cfg!(feature = "ext-zlib"),
        _ => true,
    }
}

/// Installs `resolve_feature_path` on the `$LOAD_PATH` array.
///
/// A SINGLETON method, as in CRuby (`load.c` puts it on that one object):
/// `$LOAD_PATH.singleton_methods` answers `[:resolve_feature_path]` and a
/// plain `[]` does not respond to it. Defining it on `Array` would have been
/// simpler and wrong.
pub fn install_resolve_feature_path(load_path: &RubyValue) {
    let body = crate::rproc::ProcBuilder::from_rust(
        |_recv, args, _block| {
            let feature = match args.first() {
                Some(RubyValue::Str(s)) => s.lock().to_utf8_lossy().into_owned(),
                // CRuby converts through `to_str` and raises `TypeError` for
                // anything else -- including `nil`, which is NOT the "no
                // argument" case.
                Some(other) => {
                    return Err(crate::builtins::type_error!(
                        "no implicit conversion of {} into String",
                        crate::builtins::class_name_of(other)
                    ));
                }
                None => return Err(crate::dispatch::wrong_arity(0, "1")),
            };
            Ok(match resolve_feature_path(&feature) {
                Some((kind, path)) => RubyValue::Array(crate::array_new(vec![
                    RubyValue::Symbol(crate::Symbol::intern(kind)),
                    path,
                ])),
                None => RubyValue::Nil,
            })
        },
        RubyValue::Nil,
        1,
        true,
    )
    .build();
    // The BOOT install, which fires no `singleton_method_added`: CRuby
    // defines this row in C during VM init, so no program sees it appear.
    crate::runtime_meta::install_boot_singleton(
        load_path,
        crate::Symbol::intern("resolve_feature_path"),
        body,
    );
}

/// The path `feature` names on DISK for box `box_id`, or `None`.
///
/// CRuby's own resolution: a path that is absolute or explicitly relative
/// (`./`, `../`) stands alone; every other spelling is tried under each
/// `$LOAD_PATH` root in order. A spelling with no `.rb` suffix is tried
/// with one first, which is what makes `require "json"` and `require
/// "json.rb"` the same feature.
pub fn resolve_on_disk(feature: &str, box_id: u32, append_rb: bool) -> Option<std::path::PathBuf> {
    let spellings = |base: std::path::PathBuf| -> Vec<std::path::PathBuf> {
        if feature.ends_with(".rb") || !append_rb {
            vec![base]
        } else {
            vec![base.with_extension("rb"), base]
        }
    };
    let first_readable = |candidates: Vec<std::path::PathBuf>| {
        candidates
            .into_iter()
            .find(|p| p.is_file())
            .and_then(|p| p.canonicalize().ok())
    };
    if feature.starts_with('/') || feature.starts_with("./") || feature.starts_with("../") {
        return first_readable(spellings(std::path::PathBuf::from(feature)));
    }
    let RubyValue::Array(roots) = crate::globals::global_get(box_id, "$LOAD_PATH") else {
        return None;
    };
    let roots: Vec<String> = roots
        .lock()
        .iter()
        .filter_map(|r| match r {
            RubyValue::Str(s) => Some(s.lock().to_utf8_lossy().into_owned()),
            _ => None,
        })
        .collect();
    // MAIN searches bundled-gem roots too: a feature the compile carried
    // never reaches this walk -- the unit table and `$LOADED_FEATURES`
    // answer first, and `load_from_disk` rechecks the canonical path --
    // while a file the compile never saw (a C extension's own
    // `rb_require` of a gem lib file is the shape) has only the disk.
    // A `Ruby::Box`'s require still skips them: the box would compile a
    // second, half-native copy of a gem the program carries.
    roots
        .into_iter()
        .filter(|root| box_id == 0 || !crate::globals::bundled_root(root))
        .find_map(|root| first_readable(spellings(std::path::Path::new(&root).join(feature))))
}

/// The suffixes a compiled extension wears on this platform, `DLEXT` first.
///
/// `.so` is tried on macOS too: mkmf builds `.bundle`, but a gem that ships a
/// prebuilt binary, or one built by another ruby, may carry either -- and
/// CRuby's own `search_required` walks a suffix LIST rather than one name.
#[cfg(target_vendor = "apple")]
const NATIVE_SUFFIXES: &[&str] = &["bundle", "so"];
#[cfg(not(target_vendor = "apple"))]
const NATIVE_SUFFIXES: &[&str] = &["so"];

/// Whether this process publishes the C API an extension resolves against.
///
/// One `RTLD_DEFAULT` lookup of a symbol every extension needs. Asked once
/// per process: the answer is a property of how the binary was LINKED.
fn c_api_is_published() -> bool {
    static PUBLISHED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *PUBLISHED.get_or_init(|| {
        let Ok(name) = std::ffi::CString::new("rb_define_method") else {
            return false;
        };
        // SAFETY: a NUL-terminated name against the process's own tables; a
        // miss answers null.
        !unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) }.is_null()
    })
}

/// Whether `path` wears a compiled extension's suffix on this platform.
fn is_native_library(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| NATIVE_SUFFIXES.contains(&e))
}

/// The compiled extension `feature` names on a `$LOAD_PATH` root, and the
/// stem its `Init_` is named after.
///
/// mkmf writes both from one string: `create_makefile("bcrypt_ext")` builds
/// `bcrypt_ext.bundle` and the extension defines `Init_bcrypt_ext`. So the
/// file stem IS the init stem, and no table has to carry the pair.
///
/// A feature written WITH a native suffix names the file exactly; one without
/// is tried under each suffix in turn.
pub fn resolve_native_on_disk(feature: &str, box_id: u32) -> Option<(std::path::PathBuf, String)> {
    let named = |p: std::path::PathBuf| -> Option<(std::path::PathBuf, String)> {
        let stem = p.file_stem()?.to_str()?.to_string();
        Some((p, stem))
    };
    if NATIVE_SUFFIXES
        .iter()
        .any(|s| feature.ends_with(&format!(".{s}")))
    {
        return resolve_on_disk(feature, box_id, false).and_then(named);
    }
    NATIVE_SUFFIXES
        .iter()
        .find_map(|s| resolve_on_disk(&format!("{feature}.{s}"), box_id, false))
        .and_then(named)
}

/// `dlopen` the compiled extension `feature` names, and run its `Init_`.
///
/// `None` means nothing on the load path supplies it; the caller then raises
/// the `LoadError` it would have raised anyway. This is the run-time half of
/// what a LITERAL `require` of a store gem's extension gets at compile time
/// (`parse::loader::cext`), for the spelling no compile can see -- the
/// `%w[...].each { |f| require f }` idiom over a native name.
pub fn load_native_from_disk(feature: &str, box_id: u32) -> Option<Result<bool, Signal>> {
    let (path, init) = resolve_native_on_disk(feature, box_id)?;
    let entry = path.to_string_lossy().into_owned();
    // A compiled program publishes the C API only when its compile SAW a
    // require that loads an extension (`backend::link`'s `loads_cext`, a flag
    // rather than always-on because the export table is what `-dead_strip`
    // prunes against). A computed require is invisible to that decision, so
    // the check happens here -- otherwise `dlopen` fails with an unresolved
    // `rb_*` and the reason is the dynamic loader's, not zeo's.
    if !c_api_is_published() {
        return Some(Err(crate::builtins::load_error!(
            "cannot load such file -- {feature}: this program does not publish the C API, so \
             the extension at {entry} cannot resolve against it (a require zeo can see at \
             compile time publishes it; a computed one cannot)"
        )));
    }
    // Only a shared object zeo built is ever dlopened. The load path is full
    // of the other kind -- the extension `bundle install` compiled for CRuby
    // sits beside the gem's Ruby -- and that is machine code against CRuby's
    // object layout, which runs until its first field read and then faults
    // with no name for what went wrong. zeo's build leaves a sidecar beside
    // every product it links (`zeo::cext::mark_built`); its absence is the
    // honest answer, and a `rescue LoadError` around the require then takes
    // the gem's own fallback exactly as it would on a ruby without the
    // extension.
    if !built_by_zeo(&path) {
        return Some(Err(crate::builtins::load_error!(
            "cannot load such file -- {feature}: {entry} was not built by zeo, and a shared \
             object built for another Ruby's ABI cannot load (zeo compiles a gem's extension \
             from its source when the gem store is visible to the compile)"
        )));
    }
    Some(dlopen_extension(&entry, &init).inspect(|&loaded| {
        // `$LOADED_FEATURES` names the LIBRARY, which is what makes the second
        // require answer `false`. `cext::load` is idempotent by path, so the
        // two records cannot drift.
        if loaded {
            feature_loaded(box_id, &entry, "");
        }
    }))
}

/// Whether `product` carries the `<product>.zeo` sidecar zeo's extension
/// build writes beside everything it links. The same rule the compile-time
/// loader applies to a `-I` root (`parse::loader::resolve`).
pub fn built_by_zeo(product: &std::path::Path) -> bool {
    let mut name = product.file_name().unwrap_or_default().to_os_string();
    name.push(".zeo");
    product.with_file_name(name).is_file()
}

/// The C-API crate loads it, when this binary carries one. A binary
/// without it exports no `rb_*` either, so `c_api_is_published` has
/// already refused above; the hook still answers a LoadError of its own.
fn dlopen_extension(entry: &str, init: &str) -> Result<bool, Signal> {
    crate::capi_hooks::load(entry, init)
}

/// The `--embed-sources` pack: the ruby source that travelled inside the
/// program, keyed by the load-path-relative spelling a `require` writes.
static SOURCES: std::sync::OnceLock<HashMap<&'static str, &'static str>> =
    std::sync::OnceLock::new();

/// Installs the pack. Emitted once, at startup, beside the feature units.
pub fn install_sources(rows: &'static [(&'static str, &'static str)]) {
    let _ = SOURCES.set(rows.iter().copied().collect());
}

/// The embedded source `feature` names, with the spelling it answers to.
///
/// The pack is consulted BEFORE disk: a hermetic binary answers the same way
/// wherever it runs, which is the point of embedding at all.
fn resolve_embedded(feature: &str) -> Option<(&'static str, &'static str)> {
    let pack = SOURCES.get()?;
    let bare = feature.strip_suffix(".rb").unwrap_or(feature);
    pack.get_key_value(bare).map(|(k, v)| (*k, *v))
}

/// Compile and run the file `feature` names on disk, in box `box_id`.
///
/// `None` means it resolved to nothing -- the caller raises the `LoadError`
/// it would have raised anyway. `Some(Ok(false))` is CRuby's answer for a
/// file already loaded, or one loading further up the stack (which is what
/// makes a require CYCLE terminate rather than recurse). `reload` is
/// `Kernel#load`'s rule: run it again, and answer `true` either way.
pub fn load_from_disk(feature: &str, box_id: u32, reload: bool) -> Option<Result<bool, Signal>> {
    tracing::debug!(feature, box_id, reload, "require from disk");
    // `load` names an exact file; `require` appends `.rb` to a suffix-less
    // spelling, which is what makes `require "json"` and `require "json.rb"`
    // one feature.
    // The embedded pack first, then disk -- see `resolve_embedded`.
    let (path, embedded) = match resolve_embedded(feature) {
        Some((name, text)) => (format!("<embedded>/{name}.rb"), Some(text)),
        None => {
            let found = resolve_on_disk(feature, box_id, !reload)?;
            // A compiled extension is not source. `require "probe.bundle"`
            // resolves here verbatim, and reading it as text answered
            // "stream did not contain valid UTF-8" -- so hand it back and let
            // `load_native_from_disk` dlopen it.
            if is_native_library(&found) {
                return None;
            }
            // A file this program compiled in as a unit is that unit, however
            // the require spelled it: an `autoload` naming it on the load path
            // and a `require_relative` naming it by path load ONE file. A
            // second compiled copy would define a second class the compiled
            // code never reads.
            if !reload {
                let canonical = std::fs::canonicalize(&found).ok();
                for spelling in std::iter::once(found.as_path()).chain(canonical.as_deref()) {
                    if let Some(result) = load_feature(&spelling.to_string_lossy()) {
                        return Some(result);
                    }
                }
            }
            (found.to_string_lossy().into_owned(), None)
        }
    };
    let key = (box_id, path.clone());
    if !reload {
        let mut st = state().lock();
        if st.disk_loaded.contains(&key) || !st.disk_loading.insert(key.clone()) {
            return Some(Ok(false));
        }
    }
    let Some(compiler) = UNIT_COMPILER.get() else {
        state().lock().disk_loading.remove(&key);
        return Some(Err(crate::builtins::not_impl_error!(
            "this program was compiled without the unit compiler, so it cannot load `{feature}`              at run time (zeo links it only into a program it can see reach a computed require)"
        )));
    };
    let source = match embedded {
        Some(text) => text.to_string(),
        None => match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                state().lock().disk_loading.remove(&key);
                return Some(Err(crate::builtins::load_error!(
                    "cannot load such file -- {feature} ({e})"
                )));
            }
        },
    };
    let result = compiler.load(&source, &path, box_id);
    let mut st = state().lock();
    st.disk_loading.remove(&key);
    match result {
        // A file that raised is NOT loaded: CRuby leaves it out of
        // `$LOADED_FEATURES` so a later require retries it.
        Err(e) => Some(Err(e)),
        Ok(_) => {
            st.disk_loaded.insert(key);
            drop(st);
            if !reload {
                crate::globals::append_loaded_feature_in(box_id, &path);
            }
            Some(Ok(true))
        }
    }
}

static UNITS: std::sync::OnceLock<HashMap<&'static str, UnitImpl>> = std::sync::OnceLock::new();

#[derive(Default)]
struct LoadState {
    /// Keyed by UNIT IDENTITY (the fn pointer), not the feature spelling: one
    /// unit registers under several spellings (the load-path-relative name AND
    /// the absolute path), and a per-spelling table ran the same file once per
    /// spelling -- rspec-core's exception_presenter loaded through both and
    /// its `PENDING_DETAIL_FORMATTER =` executed twice, warning where CRuby
    /// (one entry per FILE in `$LOADED_FEATURES`) is silent.
    loaded: HashSet<usize>,
    loading: HashSet<usize>,
    /// How many unit loads are on the stack right now.
    depth: u32,
    /// Autoload targets declared DURING a unit load, run when the outermost
    /// load returns -- see [`defer_autoload_target`].
    autoload_queue: Vec<String>,
    /// The same two sets for a file loaded from DISK at run time, keyed by
    /// `(box, canonical path)`. The path is the identity CRuby's
    /// `$LOADED_FEATURES` uses and the only one available for a file no
    /// unit was compiled for; the BOX is what makes a file re-execute per
    /// box, which is CRuby's own rule and falls out of each box having its
    /// own `$LOADED_FEATURES`.
    disk_loaded: HashSet<(u32, String)>,
    disk_loading: HashSet<(u32, String)>,
}

fn state() -> &'static parking_lot::Mutex<LoadState> {
    static S: std::sync::LazyLock<parking_lot::Mutex<LoadState>> =
        std::sync::LazyLock::new(Default::default);
    &S
}

/// Installs the program's units. Emitted once, at startup, before `main`'s
/// first statement -- a require in the very first line must already see them.
pub fn install_feature_units(rows: &'static [(&'static str, UnitFn)]) {
    let _ = UNITS.set(collect_units(
        rows.iter().map(|&(n, f)| (n, UnitImpl::Rust(f))),
    ));
}

/// [`install_feature_units`] for a Cranelift-emitted program.
pub fn install_feature_units_c(rows: &'static [(&'static str, CUnitFn)]) {
    let _ = UNITS.set(collect_units(
        rows.iter().map(|&(n, f)| (n, UnitImpl::C(f))),
    ));
}

/// The FIRST row claiming a spelling wins. Rows arrive in demand order, so
/// first-wins is the earliest require's answer -- and it is what makes the
/// compiler's collision warning true (`analyze::warn_on_colliding_unit_features`
/// names the loser). Collecting straight into the map silently let the LAST
/// row win instead, which is how pub_grub's `rubygems.rb` came to answer
/// `require "rubygems"`.
fn collect_units(
    rows: impl Iterator<Item = (&'static str, UnitImpl)>,
) -> HashMap<&'static str, UnitImpl> {
    let mut map: HashMap<&'static str, UnitImpl> = HashMap::new();
    for (name, imp) in rows {
        map.entry(name).or_insert(imp);
    }
    map
}

/// The key a feature string resolves under: the `.rb` suffix is optional in
/// every `require`, so it is not part of the identity.
fn key(feature: &str) -> &str {
    feature.strip_suffix(".rb").unwrap_or(feature)
}

/// Runs `feature`'s unit if this program compiled one in.
///
/// `None` means no such unit -- the caller raises the LoadError it would have
/// raised anyway. `Some(Ok(false))` is CRuby's answer for a feature already
/// loaded (or one loading further up the stack: a cycle).
pub fn load_feature(feature: &str) -> Option<Result<bool, Signal>> {
    let name = key(feature);
    let Some(&unit) = UNITS.get()?.get(name) else {
        crate::trace::trace!(
            crate::trace::Topic::Unit,
            state().lock().depth,
            "no unit for {name:?}"
        );
        return None;
    };
    let identity = unit.identity();
    let depth = {
        let mut st = state().lock();
        if st.loaded.contains(&identity) || !st.loading.insert(identity) {
            crate::trace::trace!(
                crate::trace::Topic::Unit,
                st.depth,
                "{name:?} -> false (already {})",
                match st.loaded.contains(&identity) {
                    true => "loaded",
                    false => "loading: a cycle",
                }
            );
            return Some(Ok(false));
        }
        st.depth += 1;
        st.depth - 1
    };
    crate::trace::trace!(crate::trace::Topic::Unit, depth, "run {name:?}");
    let result = unit.call();
    let outermost;
    let outcome = {
        let mut st = state().lock();
        st.loading.remove(&identity);
        st.depth -= 1;
        outermost = st.depth == 0;
        match result {
            // A unit that raised is NOT loaded: CRuby leaves the feature out
            // of `$LOADED_FEATURES` so a later require retries it.
            Err(e) => {
                crate::trace::trace!(crate::trace::Topic::Unit, st.depth, "{name:?} RAISED");
                Some(Err(e))
            }
            Ok(_) => {
                st.loaded.insert(identity);
                crate::trace::trace!(crate::trace::Topic::Unit, st.depth, "{name:?} -> true");
                Some(Ok(true))
            }
        }
        // The guard drops HERE, before the drain below re-locks the state --
        // holding it across `drain_autoload_queue` was a self-deadlock.
    };
    // `$LOADED_FEATURES` is NOT recorded here. The unit's own body leads with
    // the `FeatureLoaded` marker every spliced file carries
    // (`parse::loader::splice`), which records the file's CANONICAL ABSOLUTE
    // PATH -- and records it BEFORE the body runs, which is CRuby's own order
    // (`rb_provide_feature` precedes evaluation, and that is what makes a
    // require cycle answer false rather than recurse).
    //
    // Appending again here wrote a SECOND entry for one load, under the
    // spelling the require asked with rather than the file it reached. Ruby
    // lists one entry per file, and real libraries read that list as paths:
    // rubygems' `already_loaded?` compares `"#{load_path_entry}/#{file}"`
    // against it, and bundler's `shared_helpers` asks each entry
    // `start_with?(resolved_path)`. A bare feature name satisfies neither.
    if outermost {
        drain_autoload_queue();
    }
    outcome
}

/// Queues an autoload target declared while a unit load is on the stack; the
/// queue drains when the OUTERMOST load returns. An autoload's declarer (and
/// everything up its require chain) must finish executing before the target
/// runs -- rspec-expectations' `built_in.rb` declares `autoload :Has` while
/// `matchers.rb` (which defines the `HAS_REGEX` the target reads) is still
/// mid-execution. CRuby loads at first constant ACCESS, which is always after
/// the declaring require graph completes; end-of-outermost-load is the closest
/// point this eager model has. Answers whether the target was queued -- at
/// depth 0 (eager main-line code) the caller keeps its immediate load.
pub fn defer_autoload_target(feature: &str) -> bool {
    let mut st = state().lock();
    if st.depth == 0 {
        return false;
    }
    st.autoload_queue.push(feature.to_string());
    true
}

/// Loads everything [`defer_autoload_target`] queued. Runs at outermost-load
/// return, looping because a drained target's own unit may declare more
/// autoloads. A target whose load raises -- ANY class, not just `LoadError`
/// -- rolls back to pending: CRuby runs nothing until the constant's first
/// ACCESS, so "declared, never loaded" is exactly its state (rspec-core's
/// bisect formatter drags in drb, and only a `--bisect` run ever touches it).
/// `load_feature` leaves a failed unit retryable, so a target the program
/// genuinely reaches still surfaces its error at that reach.
fn drain_autoload_queue() {
    loop {
        let next = {
            let mut st = state().lock();
            if st.autoload_queue.is_empty() {
                return;
            }
            st.autoload_queue.remove(0)
        };
        let _ = load_feature(&next);
    }
}

/// Whether `feature` names a unit this program compiled in -- asked before a
/// LoadError is raised, and by `autoload` to decide whether it can serve the
/// registration it just recorded.
pub fn has_feature(feature: &str) -> bool {
    UNITS.get().is_some_and(|u| u.contains_key(key(feature)))
}

/// Whether this program compiled a load path in at all -- i.e. whether some
/// file in it computes a require/autoload target. Only then is a feature that
/// is missing from the table a real failure: everywhere else, `autoload` keeps
/// its documented register-but-never-load behaviour, which is what CRuby's own
/// laziness looks like from the outside until the constant is referenced.
pub fn any_units() -> bool {
    UNITS.get().is_some_and(|u| !u.is_empty())
}

static DECLINED: std::sync::OnceLock<HashMap<&'static str, &'static str>> =
    std::sync::OnceLock::new();

/// Files on a compiled-in load path that zeo could not lower, with the reason.
///
/// A load path is compiled in WHOLE, so it includes files the program may well
/// never require -- a gem's optional adapters, its test helpers. One of those
/// hitting a lowering gap must not fail the build for code that never runs, but
/// it must not vanish either: the feature is recorded here, and requiring it
/// raises `LoadError` naming the gap. Loud at the point of use, which is the
/// only place the difference is observable.
pub fn install_declined_features(rows: &'static [(&'static str, &'static str)]) {
    let _ = DECLINED.set(rows.iter().copied().collect());
}

/// Why `feature` is not available, for the `LoadError` message.
pub fn decline_reason(feature: &str) -> Option<&'static str> {
    DECLINED.get()?.get(key(feature)).copied()
}

/// A `require` succeeded, at its own document position. `entry` joins this
/// box's `$LOADED_FEATURES` (CRuby's `rb_provide_feature`, which runs BEFORE
/// the file it names) and `feature`, when non-empty, activates a
/// statically linked extension: its require-gated rows become answerable and
/// its gated constants become visible from here on, not from line 1.
/// `require "<a library the runtime carries>"`, answered at RUN time.
///
/// `None` for a feature zeo does not compile in; the caller goes on to the
/// disk search and then to its `LoadError`.
///
/// This is the same answer the front end folds a LITERAL require of a builtin
/// into (`lower::calls` emits a `FeatureLoaded` marker and a boolean, and
/// nothing else), which is what keeps the two spellings agreeing. It has to
/// exist separately because a computed require names no feature the compiler
/// could see: `f = "date"; require f` reaches here instead.
///
/// The boolean is ruby's: `true` the first time, `false` for a feature
/// already recorded -- and `false` even on the first require of one CRuby
/// loads before line 1.
pub fn load_builtin_feature(feature: &str, box_id: u32) -> Option<bool> {
    if !zeo_abi::is_builtin_feature(feature) || !build_carries_ext(feature) {
        return None;
    }
    let canonical = zeo_abi::canonical_ext_feature(feature);
    let entry = format!("<zeo-builtin>/{canonical}.rb");
    let first = !crate::globals::loaded_feature_recorded(box_id, &entry);
    feature_loaded(box_id, &entry, canonical);
    Some(first && !zeo_abi::PRELOADED_AT_BOOT.contains(&feature))
}

pub fn feature_loaded(box_id: u32, entry: &str, feature: &str) {
    // Idempotent: a second `require` of a feature records nothing. The
    // compiler splices a file once, but a require written twice under two
    // different guards reaches this twice -- and a feature ruby has loaded
    // before line 1 is already in the seed.
    if !crate::globals::loaded_feature_recorded(box_id, entry) {
        crate::globals::append_loaded_feature_in(box_id, entry);
    }
    if !feature.is_empty() {
        crate::builtins::gate::activate(feature);
        crate::constants::reveal_feature_classes(feature);
    }
}

#[cfg(test)]
mod tests {
    /// The predicate `load_builtin_feature` gates on. It lives in `zeo_abi`
    /// so the compiler's fold and this run-time answer read ONE list --
    /// a second copy would drift, and the drift reads as a `LoadError` for a
    /// library the binary is carrying.
    #[test]
    fn the_builtin_feature_list_has_one_owner() {
        for f in zeo_abi::NATIVE_FEATURES {
            assert!(
                zeo_abi::is_builtin_feature(f),
                "{f} is in NATIVE_FEATURES and the predicate refuses it"
            );
        }
        for f in zeo_abi::ext_feature_names() {
            assert!(
                zeo_abi::is_builtin_feature(f),
                "{f} is an ext feature and the predicate refuses it"
            );
        }
        assert!(!zeo_abi::is_builtin_feature("no_such_library_anywhere"));
    }

    /// The alias spellings a `require` may write all collapse to one feature,
    /// so requiring `yaml` and `psych` is requiring one thing.
    #[test]
    fn an_alias_spelling_canonicalizes_to_its_feature() {
        for (spelling, canonical) in [
            ("cgi", "cgi/escape"),
            ("cgi/util", "cgi/escape"),
            ("yaml", "psych"),
            // An ALGORITHM is its own feature: `Digest::SHA256` arrives with
            // `digest/sha2`, not with `digest`.
            ("digest/sha2", "digest/sha2"),
            ("digest/bubblebabble", "digest"),
            ("digest", "digest"),
            ("stringio", "stringio"),
        ] {
            assert_eq!(zeo_abi::canonical_ext_feature(spelling), canonical);
        }
    }

    /// A feature ruby loads before line 1 answers `false` even the first
    /// time, which is what makes `require "set"` agree with the oracle.
    #[test]
    fn a_preloaded_feature_is_a_builtin_that_answers_false() {
        for f in zeo_abi::PRELOADED_AT_BOOT {
            assert!(
                zeo_abi::is_builtin_feature(f),
                "{f} is preloaded but not a builtin, so nothing would load it"
            );
        }
    }

    /// A feature zeo does not carry is not this entry's business: `None`
    /// sends the caller on to the disk search and then to its `LoadError`.
    #[test]
    fn a_feature_zeo_does_not_carry_is_declined() {
        assert_eq!(
            super::load_builtin_feature("no_such_library_anywhere", 0),
            None
        );
        assert_eq!(super::load_builtin_feature("./a/relative/path", 0), None);
    }
}
