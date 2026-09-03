#[test]
fn external_gem_store_resolves_pure_ruby_and_excludes_native() {
    // `--gem-path` + `--lockfile` against a self-contained fixture
    // store (tests/fixtures/gem_store/store). Covers all three provider
    // paths: a pure-Ruby gem resolves and compiles; a gem shipping its C as
    // SOURCE is compiled from it; a precompiled-platform-only gem is
    // excluded, with a reason recorded in the disclosure report.
    let store = crate::support::gem_store("ffi");
    let report = std::env::temp_dir().join(format!("zeo-store-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&report);
    let opts = zeo::CompileOptions {
        gem_paths: vec![store.clone()],
        lockfile: Some(store.join("Gemfile.lock")),
        gem_report: Some(report.clone()),
        ..Default::default()
    };
    zeo::check_program_with("require \"purelib\"\nputs Purelib::VERSION\n", &opts)
        .expect("a pure-Ruby store gem resolves and compiles");
    let json = std::fs::read_to_string(&report).unwrap();
    let _ = std::fs::remove_file(&report);
    assert!(
        json.contains(r#""purelib": {"by": "bundled-gem""#),
        "{json}"
    );
    assert!(
        json.contains(r#""precompiled": {"by": null, "excluded": "precompiled-platform-gem""#),
        "{json}"
    );
}

/// A gem that ships its C as SOURCE is COMPILED, loaded and called.
///
/// The whole path in one test: the gemspec's `extensions` names an
/// `extconf.rb`, zeo runs it, mkmf writes a Makefile, zeo compiles and links
/// without `make`, and the program dlopens the result and calls a method
/// `Init_nativelib` defined. Nothing short of running it proves the last
/// three steps.
#[test]
fn a_store_gem_shipping_c_source_is_compiled_and_loaded() {
    if std::process::Command::new("cc")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipping: this machine has no C compiler");
        return;
    }
    let store = crate::support::gem_store("ffi");
    let opts = zeo::CompileOptions {
        gem_paths: vec![store.clone()],
        lockfile: Some(store.join("Gemfile.lock")),
        ..Default::default()
    };
    let out = crate::support::compile_link_run(
        "require \"nativelib\"\np Nativelib::BUILT\nputs Nativelib.greet\n",
        &opts,
        &[],
        &[],
    );
    assert_eq!(out.stdout, "true\nhello from C\n", "stderr: {}", out.stderr);
}

/// The build is OUT OF TREE. A gem store is shared and often read only, so a
/// build that wrote into it would leave one project's artifacts where
/// another project reads them -- and would dirty this fixture in the repo.
#[test]
fn building_an_extension_leaves_the_gem_store_untouched() {
    if std::process::Command::new("cc")
        .arg("--version")
        .output()
        .is_err()
    {
        eprintln!("skipping: this machine has no C compiler");
        return;
    }
    let store = crate::support::gem_store("ffi");
    let ext = store.join("gems/nativelib-1.0.0/ext/nativelib");
    let before: Vec<String> = listing(&ext);
    let opts = zeo::CompileOptions {
        gem_paths: vec![store.clone()],
        lockfile: Some(store.join("Gemfile.lock")),
        ..Default::default()
    };
    zeo::check_program_with("require \"nativelib\"\n", &opts).expect("the extension builds");
    assert_eq!(
        listing(&ext),
        before,
        "the build wrote into the gem store; it must stage into the cache"
    );
}

fn listing(dir: &std::path::Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// The lockfile picks the version, not the store.
///
/// The fixture store holds purelib 1.0.0 AND 2.0.0 while the lockfile pins
/// 1.0.0, so a resolver that took the newest -- which is what `require` does
/// in a plain RubyGems process -- would answer 2.0.0. This compiles and RUNS
/// the program, because reaching codegen only proves a version resolved, not
/// which one ended up in the binary.
#[test]
fn the_lockfile_selects_the_version_when_the_store_holds_several() {
    let store = crate::support::gem_store("ffi");
    let opts = zeo::CompileOptions {
        gem_paths: vec![store.clone()],
        lockfile: Some(store.join("Gemfile.lock")),
        ..Default::default()
    };
    let out = crate::support::compile_link_run(
        "require \"purelib\"\nputs Purelib::VERSION\n",
        &opts,
        &[],
        &[],
    );

    assert_eq!(
        out.stdout, "1.0.0\n",
        "the store also holds purelib 2.0.0; the lockfile pins 1.0.0"
    );
}

// -- The fiber-backed Enumerator (per CRuby's enumerator.c).
// Every positive expectation below is oracle-verified against ruby 4.0.6.

// --- A runtime-installed singleton method threads its call-site block
// through ProcData, so `yield`/`block_given?`/`&blk` work inside a
// per-object singleton (`def obj.m`, `class << obj`) and a `def` in a
// Class.new block.

// A variadic `attach_function [.., :varargs]` builds its call interface at
// runtime through libffi: `snprintf` formats mixed int/string/double varargs
// into a buffer (and a call with no varargs at all still works). Oracle-pinned
// against ruby 4.0.6 + the real `ffi` gem.

// A `callback` type + a Ruby Proc passed for that argument becomes a libffi
// closure the C side calls back into: `qsort` and `bsearch` are driven by Ruby
// comparators, ascending and descending.

// An exception raised inside a callback cannot unwind through the C frames, so
// it is stashed and re-raised once the C function returns.
