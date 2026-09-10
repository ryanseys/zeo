//! The store tier: `zeo install` precompiles a locked gem into the gem
//! store, `zeo gem precompile` ships one inside a platform gem, and a
//! store-active compile links the artifact instead of splicing the source.

use super::*;

/// The store tier end to end: `zeo install` precompiles a locked gem into
/// the gem store, and a store-active compile LINKS that artifact instead
/// of splicing the gem's source. The proof is a poisoned source tree --
/// only the artifact carries the working body, so the answer can only
/// come from the link. Removing the artifact flips the same compile back
/// to the source splice, which is the fallback contract.
/// A one-gem project against a scratch store, RubyGems-shaped: the
/// `tinygem` sources, its gemspec, and a locked Gemfile beside an app
/// that requires it. Answers `(store, proj)`.
fn tinygem_project(dir: &Path) -> (PathBuf, PathBuf) {
    let store = dir.join("store");
    let gem_lib = store.join("gems/tinygem-1.0.0/lib");
    std::fs::create_dir_all(store.join("specifications")).expect("mkdir");
    std::fs::create_dir_all(&gem_lib).expect("mkdir");
    std::fs::write(
        store.join("specifications/tinygem-1.0.0.gemspec"),
        "Gem::Specification.new do |s|\n  s.name = \"tinygem\"\n  s.version = \"1.0.0\"\n  \
         s.require_paths = [\"lib\"]\nend\n",
    )
    .expect("write gemspec");
    let good = "class Tinygem\n  def greet = \"hello from tinygem\"\nend\n";
    std::fs::write(gem_lib.join("tinygem.rb"), good).expect("write lib");
    let proj = dir.join("proj");
    std::fs::create_dir_all(&proj).expect("mkdir");
    std::fs::write(
        proj.join("Gemfile"),
        "source \"https://rubygems.org\"\ngem \"tinygem\"\n",
    )
    .expect("write Gemfile");
    std::fs::write(
        proj.join("Gemfile.lock"),
        "GEM\n  remote: https://rubygems.org/\n  specs:\n    tinygem (1.0.0)\n\nPLATFORMS\n  \
         ruby\n\nDEPENDENCIES\n  tinygem\n\nBUNDLED WITH\n   4.0.18\n",
    )
    .expect("write lock");
    std::fs::write(
        proj.join("app.rb"),
        "require \"tinygem\"\nputs Tinygem.new.greet\n",
    )
    .expect("write app");
    (store, proj)
}

#[test]
fn zeo_install_populates_the_store_and_a_compile_links_it() {
    let dir = scratch("install");
    let (store, proj) = tinygem_project(&dir);
    let gem_lib = store.join("gems/tinygem-1.0.0/lib");

    let report = ok(zeo()
        .current_dir(&proj)
        .arg("install")
        .arg("--gem-path")
        .arg(&store)
        .env("ZEO_CACHE", "0"));
    assert!(report.contains("install tinygem 1.0.0"), "report: {report}");
    let artifacts: Vec<PathBuf> = walk_files(&store.join("zeo"));
    assert_eq!(
        artifacts.len(),
        1,
        "one artifact at its store home: {artifacts:?}"
    );
    assert!(artifacts[0].ends_with("tinygem-1.0.0/pkg.zeopkg"));

    // Only the ARTIFACT has the working body now.
    std::fs::write(
        gem_lib.join("tinygem.rb"),
        "raise \"the SOURCE was spliced\"\n",
    )
    .expect("poison lib");
    let compile = |out: &str| {
        let bin = proj.join(out);
        zeo()
            .current_dir(&proj)
            .arg("build")
            .arg("app.rb")
            .arg("--gem-path")
            .arg(&store)
            .arg("--bundle-gemfile")
            .arg("Gemfile")
            .arg("-o")
            .arg(&bin)
            .env("ZEO_CACHE", "0")
            .output()
            .expect("spawn");
        bin
    };
    let linked = compile("app-linked");
    let out = Command::new(&linked).output().expect("run");
    assert!(
        out.status.success(),
        "the artifact answers: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello from tinygem\n");

    // No artifact -> the same compile splices the source again.
    std::fs::remove_file(&artifacts[0]).expect("remove artifact");
    let spliced = compile("app-spliced");
    let out = Command::new(&spliced).output().expect("run");
    assert!(!out.status.success(), "the poisoned source raises");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("the SOURCE was spliced"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `zeo flags` prints the handoff line the compile itself consumes: the
/// store, the Gemfile, and -- once `zeo install` has run -- the artifact.
/// The line is shell-quoted for `$(zeo flags)`; `--json` is the tools'
/// form and carries the same artifact path.
#[test]
fn zeo_flags_prints_the_projects_handoff_line() {
    let dir = scratch("flags");
    let (store, proj) = tinygem_project(&dir);

    let before = ok(zeo()
        .current_dir(&proj)
        .arg("flags")
        .arg("--gem-path")
        .arg(&store));
    assert!(before.contains("--gem-path '"), "line: {before}");
    assert!(before.contains("--bundle-gemfile '"), "line: {before}");
    assert!(
        !before.contains("--with-package"),
        "no artifact before install: {before}"
    );

    ok(zeo()
        .current_dir(&proj)
        .arg("install")
        .arg("--gem-path")
        .arg(&store)
        .env("ZEO_CACHE", "0"));
    let after = ok(zeo()
        .current_dir(&proj)
        .arg("flags")
        .arg("--gem-path")
        .arg(&store));
    assert!(
        after.contains("--with-package '") && after.contains("tinygem-1.0.0/pkg.zeopkg'"),
        "the installed artifact rides the line: {after}"
    );
    let json = ok(zeo()
        .current_dir(&proj)
        .arg("flags")
        .arg("--json")
        .arg("--gem-path")
        .arg(&store));
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid json");
    let artifact = parsed["gems"][0]["artifact"].as_str().expect("an artifact");
    assert!(artifact.ends_with("tinygem-1.0.0/pkg.zeopkg"), "{artifact}");
}

/// Every file under `root`, recursively.
fn walk_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk_files(&path));
        } else {
            out.push(path);
        }
    }
    out
}

/// The shipping tier end to end: `zeo gem precompile` builds a platform
/// gem with the artifact inside (through RubyGems' own Gem::Package),
/// `zeo gem install` unpacks it into a store, `zeo install` PROMOTES the
/// shipped artifact instead of compiling, and a store-active compile
/// links it. The greeting names the artifact so a source splice cannot
/// fake the answer once the tree is poisoned.
#[test]
#[ignore = "builds a real gem from source; run it with --run-ignored"]
fn a_precompiled_platform_gem_ships_its_artifact_to_a_consumer() {
    let dir = scratch("shipping");
    let gem_dir = dir.join("shipgem");
    std::fs::create_dir_all(gem_dir.join("lib")).expect("mkdir");
    std::fs::write(
        gem_dir.join("shipgem.gemspec"),
        "Gem::Specification.new do |s|\n  s.name = \"shipgem\"\n  s.version = \"2.0.0\"\n  \
         s.summary = \"ships an artifact\"\n  s.authors = [\"e2e\"]\n  \
         s.files = [\"lib/shipgem.rb\"]\n  s.require_paths = [\"lib\"]\nend\n",
    )
    .expect("write gemspec");
    std::fs::write(
        gem_dir.join("lib/shipgem.rb"),
        "class Shipgem\n  def greet = \"hello from the shipped artifact\"\nend\n",
    )
    .expect("write lib");
    let report = ok(zeo()
        .current_dir(&gem_dir)
        .arg("gem")
        .arg("precompile")
        .env("ZEO_CACHE", "0"));
    assert!(
        report.contains("carries the precompiled artifact"),
        "report: {report}"
    );
    let gems: Vec<PathBuf> = std::fs::read_dir(&gem_dir)
        .expect("read gem dir")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "gem"))
        .collect();
    assert_eq!(gems.len(), 1, "one platform gem: {gems:?}");
    assert!(
        !gem_dir.join("zeo").exists(),
        "the loose artifact is cleaned"
    );

    // The consumer machine: install the .gem, then let `zeo install`
    // promote the shipped artifact -- no compile.
    let store = dir.join("store");
    // `--no-document`: rdoc's install hook is the environment's concern (rdoc 8
    // hard-requires rbs), and this test is about the artifact.
    ok(zeo()
        .arg("gem")
        .arg("install")
        .arg("--local")
        .arg("--no-document")
        .arg("--install-dir")
        .arg(&store)
        .arg(&gems[0]));
    let proj = dir.join("proj");
    std::fs::create_dir_all(&proj).expect("mkdir");
    std::fs::write(
        proj.join("Gemfile"),
        "source \"https://rubygems.org\"\ngem \"shipgem\"\n",
    )
    .expect("write Gemfile");
    std::fs::write(
        proj.join("Gemfile.lock"),
        "GEM\n  remote: https://rubygems.org/\n  specs:\n    shipgem (2.0.0)\n\nPLATFORMS\n  \
         ruby\n\nDEPENDENCIES\n  shipgem\n\nBUNDLED WITH\n   4.0.18\n",
    )
    .expect("write lock");
    std::fs::write(
        proj.join("app.rb"),
        "require \"shipgem\"\nputs Shipgem.new.greet\n",
    )
    .expect("write app");
    let report = ok(zeo()
        .current_dir(&proj)
        .arg("install")
        .arg("--gem-path")
        .arg(&store)
        .env("ZEO_CACHE", "0"));
    assert!(
        report.contains("shipped artifact"),
        "the artifact promotes without a compile: {report}"
    );

    // Poison the installed SOURCE: only the artifact can answer now.
    let installed_lib = store.join("gems").join(
        std::fs::read_dir(store.join("gems"))
            .expect("read store gems")
            .flatten()
            .map(|e| e.file_name())
            .find(|n| n.to_string_lossy().starts_with("shipgem-"))
            .expect("the installed gem dir"),
    );
    std::fs::write(
        installed_lib.join("lib/shipgem.rb"),
        "raise \"the SOURCE was spliced\"\n",
    )
    .expect("poison lib");
    let bin = proj.join("app-bin");
    ok(zeo()
        .current_dir(&proj)
        .arg("build")
        .arg("app.rb")
        .arg("--gem-path")
        .arg(&store)
        .arg("--bundle-gemfile")
        .arg("Gemfile")
        .arg("-o")
        .arg(&bin)
        .env("ZEO_CACHE", "0"));
    let out = Command::new(&bin).output().expect("run");
    assert!(
        out.status.success(),
        "the shipped artifact answers: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "hello from the shipped artifact\n"
    );
}

/// A packaged feature that is ALSO a builtin name, under an active store: the
/// require must stay a call the merged unit rows answer. `tmpdir` is the
/// shape -- zeo provides it natively, the lock names it, and an installed
/// artifact makes it a packaged feature. If the builtin arm of the
/// resolvability scan claimed a compile-time verdict for it, the require
/// would go down a splice road where the store override had already
/// dismissed the builtin: `cannot load such file -- tmpdir`.
#[test]
fn a_packaged_builtin_feature_defers_to_the_merged_unit() {
    let root = crate::paths::workspace_root();
    let store = root.join(zeo::gems::bundled::RESOLVED_TIER);
    let tmpdir_rb = store.join("gems/tmpdir-0.3.1/lib/tmpdir.rb");
    let fileutils_rb = store.join("gems/fileutils-1.8.0/lib/fileutils.rb");
    if !tmpdir_rb.is_file() || !fileutils_rb.is_file() {
        eprintln!("skipping: the resolved store has no tmpdir/fileutils (run `cargo xtask deps`)");
        return;
    }
    let dir = scratch("packaged-builtin");
    let pkg = build_named_package(&dir, "tmpdir", &tmpdir_rb);
    // tmpdir's unit requires fileutils at run time (a deferred foreign
    // feature in its manifest), so the dependency artifact merges beside
    // it -- the test must not lean on whatever the machine's store holds.
    let dep = build_named_package(&dir, "fileutils", &fileutils_rb);

    let host = dir.join("host.rb");
    std::fs::write(&host, "require \"tmpdir\"\np Dir.respond_to?(:tmpdir)\n").expect("write host");
    let bin = dir.join("host.bin");
    ok(zeo()
        .arg("build")
        .arg(&host)
        .arg("--bundle-gemfile")
        .arg(root.join("Gemfile"))
        .arg("--gem-path")
        .arg(&store)
        .arg("--with-package")
        .arg(&pkg)
        .arg("--with-package")
        .arg(&dep)
        .arg("-o")
        .arg(&bin)
        .env("ZEO_CACHE", "0"));
    assert_eq!(ok(&mut Command::new(&bin)), "true\n");
}

/// A packaged timeout must WORK, not just merge: `Timeout::GET_TIME` is a
/// module constant whose value is a runtime `Method` object written when the
/// unit runs, read from `State`'s bodies through the lexical scope -- the
/// shape that once reached the timer thread uninitialized.
#[test]
fn a_packaged_timeout_times_out() {
    let root = crate::paths::workspace_root();
    let timeout_rb = root
        .join(zeo::gems::bundled::RESOLVED_TIER)
        .join("gems/timeout-0.6.1/lib/timeout.rb");
    if !timeout_rb.is_file() {
        eprintln!("skipping: the resolved store has no timeout 0.6.1 (run `cargo xtask deps`)");
        return;
    }
    let dir = scratch("packaged-timeout");
    let pkg = build_named_package(&dir, "timeout", &timeout_rb);

    let host = dir.join("host.rb");
    std::fs::write(
        &host,
        "require \"timeout\"\n\
         p Timeout.timeout(5) { :ok }\n\
         begin\n\
           Timeout.timeout(0.05) { sleep 1 }\n\
         rescue Timeout::Error => e\n\
           p [:timed_out, e.message]\n\
         end\n",
    )
    .expect("write host");
    let bin = dir.join("host.bin");
    ok(zeo()
        .arg("build")
        .arg(&host)
        .arg("--with-package")
        .arg(&pkg)
        .arg("-o")
        .arg(&bin)
        .env("ZEO_CACHE", "0"));
    assert_eq!(
        ok(&mut Command::new(&bin)),
        ":ok\n[:timed_out, \"execution expired\"]\n"
    );
}
