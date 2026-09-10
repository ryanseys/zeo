//! The package cache: what a hit serves, what an edit invalidates, and the
//! two refusals that drop back to the source splice.

use super::*;

#[test]
fn the_package_cache_serves_hits_and_invalidates_on_edit() {
    // The machine-wide package cache, proven the only way a deterministic
    // compiler can be: MARK the cached artifact, and the next build serves
    // the mark; edit the source, and the build stops serving it.
    let dir = scratch("pkgcache");
    let cache = dir.join("cache");
    // The source sits in its own directory: the cache manifest stamps the
    // directories a compile read, so an output landing beside the source
    // would read as a change.
    let src = dir.join("src");
    std::fs::create_dir_all(&src).expect("mkdir");
    let entry = src.join("cachegem.rb");
    std::fs::write(&entry, "class Cachegem\n  def go = :one\nend\n").expect("write the gem");
    let build = |out: &Path| {
        ok(zeo()
            .arg("--package")
            .arg("cachegem")
            .arg("-o")
            .arg(out)
            .arg(&entry)
            .env("ZEO_CACHE", "1")
            .env("ZEO_PACKAGE_CACHE", &cache));
    };
    build(&dir.join("first.zeopkg"));
    let cached = std::fs::read_dir(&cache)
        .expect("the cache dir exists")
        .filter_map(Result::ok)
        .map(|e| e.path().join("pkg.zeopkg"))
        .find(|p| p.is_file())
        .expect("one cache entry");
    let (manifest, object) = zeo::packages::package::read_zeopkg(&cached).expect("read the entry");
    let marked = manifest.replace("\"iface_hash\": \"", "\"iface_hash\": \"cafe");
    assert_ne!(marked, manifest, "the mark landed");
    zeo::packages::package::write_zeopkg(&cached, &marked, &object).expect("mark the entry");

    build(&dir.join("second.zeopkg"));
    let (manifest, _) = zeo::packages::package::read_zeopkg(&dir.join("second.zeopkg"))
        .expect("read the second build");
    let listing: Vec<String> = std::fs::read_dir(&cache)
        .map(|es| {
            es.filter_map(Result::ok)
                .map(|e| {
                    let names: Vec<String> = std::fs::read_dir(e.path())
                        .map(|fs| {
                            fs.filter_map(Result::ok)
                                .map(|f| f.file_name().to_string_lossy().into_owned())
                                .collect()
                        })
                        .unwrap_or_default();
                    let rows =
                        std::fs::read_to_string(e.path().join("manifest")).unwrap_or_default();
                    format!(
                        "{}: {names:?} rows: {rows}",
                        e.file_name().to_string_lossy()
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    assert!(
        manifest.contains("\"iface_hash\": \"cafe"),
        "the second build was served from the cache; cache dirs: {listing:?}"
    );

    std::fs::write(&entry, "class Cachegem\n  def go = :two\nend\n").expect("edit the gem");
    build(&dir.join("third.zeopkg"));
    let (manifest, _) = zeo::packages::package::read_zeopkg(&dir.join("third.zeopkg"))
        .expect("read the third build");
    assert!(
        !manifest.contains("\"iface_hash\": \"cafe"),
        "the source edit invalidated the entry"
    );
}

#[test]
fn a_refused_artifact_drops_to_the_source_splice() {
    // The fallback tier: the same foreign-target artifact that REFUSES
    // when it is the only copy compiles from source when the gem is
    // resolvable -- with a warning naming the package, never silently.
    // (A host reopen does not trigger a drop: it installs at run time and
    // composes with the artifact.)
    let dir = scratch("drop");
    let artifact = dir.join("pureleaf.zeopkg");
    ok(zeo()
        .arg("--package")
        .arg("pureleaf")
        .arg("-o")
        .arg(&artifact)
        .arg(fixture_gem().join("lib/pureleaf.rb"))
        .env("ZEO_CACHE", "0"));
    let (manifest, object) = zeo::packages::package::read_zeopkg(&artifact).expect("read");
    let foreign = manifest.replace(
        &format!("\"target\": \"{}\"", target_of(&manifest)),
        "\"target\": \"wasm32-unknown-unknown\"",
    );
    assert_ne!(foreign, manifest, "the target was rewritten");
    zeo::packages::package::write_zeopkg(&artifact, &foreign, &object).expect("rewrite");
    let host = dir.join("host.rb");
    std::fs::write(
        &host,
        "require \"pureleaf\"\nclass Pureleaf\n  def extra = :host_extra\nend\n\
         p Pureleaf.new.extra\np Pureleaf.new.tagged(1)\n",
    )
    .expect("write host");
    let bin = dir.join("host-bin");
    let out = run(zeo()
        .arg("--with-package")
        .arg(&artifact)
        .arg("--gems")
        .arg(fixture_gem().parent().expect("fixtures dir"))
        .arg("-o")
        .arg(&bin)
        .arg(&host)
        .env("ZEO_CACHE", "0"));
    assert!(out.status.success(), "the drop compiles from source");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("dropping the precompiled artifact for 'pureleaf'"),
        "the drop warns by name: {err}"
    );
    assert_eq!(ok(&mut Command::new(&bin)), ":host_extra\n\"leaf-1\"\n");
}

#[test]
fn a_target_mismatch_refuses_by_name() {
    // No stable package ABI is promised: the identity contract is an exact
    // match, and a mismatch names itself instead of surfacing as a link
    // error.
    let dir = scratch("target");
    let artifact = dir.join("pureleaf.zeopkg");
    ok(zeo()
        .arg("--package")
        .arg("pureleaf")
        .arg("-o")
        .arg(&artifact)
        .arg(fixture_gem().join("lib/pureleaf.rb"))
        .env("ZEO_CACHE", "0"));
    let (manifest, object) = zeo::packages::package::read_zeopkg(&artifact).expect("read");
    let foreign = manifest.replace(
        &format!("\"target\": \"{}\"", target_of(&manifest)),
        "\"target\": \"wasm32-unknown-unknown\"",
    );
    assert_ne!(foreign, manifest, "the target was rewritten");
    zeo::packages::package::write_zeopkg(&artifact, &foreign, &object).expect("rewrite");
    let host = dir.join("host.rb");
    std::fs::write(&host, "require \"pureleaf\"\n").expect("write host");
    let out = run(zeo()
        .arg("--with-package")
        .arg(&artifact)
        .arg("-o")
        .arg(dir.join("host-bin"))
        .arg(&host)
        .env("ZEO_CACHE", "0"));
    assert!(!out.status.success(), "a foreign target must refuse");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("was compiled for wasm32-unknown-unknown"),
        "the refusal names both targets: {err}"
    );
}

/// The `target` value inside a manifest's JSON text.
fn target_of(manifest: &str) -> String {
    serde_json::from_str::<serde_json::Value>(manifest)
        .expect("a manifest parses")
        .get("target")
        .and_then(|t| t.as_str())
        .expect("a manifest names its target")
        .to_string()
}
