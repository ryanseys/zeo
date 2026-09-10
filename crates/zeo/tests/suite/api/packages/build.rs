//! Building a package: the bundle a `--package` compile writes, and the
//! bytes it writes wherever it runs.

use super::*;

#[test]
fn a_zeopkg_bundle_builds_links_and_runs() {
    // `-o pkg.zeopkg` bundles the object and the manifest into ONE file --
    // the shippable artifact -- and a host consumes it exactly like the
    // two-file spelling.
    let dir = scratch("bundle");
    let artifact = dir.join("pureleaf.zeopkg");
    ok(zeo()
        .arg("--package")
        .arg("pureleaf")
        .arg("-o")
        .arg(&artifact)
        .arg(fixture_gem().join("lib/pureleaf.rb"))
        .env("ZEO_CACHE", "0"));
    assert!(artifact.is_file(), "the bundle was written");
    assert!(
        !artifact.with_extension("zman").is_file(),
        "the bundle carries the manifest inside; no loose copy remains"
    );
    let host = dir.join("host.rb");
    std::fs::write(&host, "require \"pureleaf\"\np Pureleaf.new.tagged(5)\n").expect("write host");
    let bin = dir.join("host-bin");
    ok(zeo()
        .arg("--with-package")
        .arg(&artifact)
        .arg("-o")
        .arg(&bin)
        .arg(&host)
        .env("ZEO_CACHE", "0"));
    assert_eq!(ok(&mut Command::new(&bin)), "\"leaf-5\"\n");
}

#[test]
fn a_package_build_is_reproducible_across_directories() {
    // CI-asserted: the same gem compiled from two different checkouts
    // answers byte-identical artifacts. The virtual-root
    // respelling is what keeps the build directory out of the object's
    // rodata and the manifest's meta rows.
    let dir = scratch("repro");
    for side in ["one", "two"] {
        let copy = dir.join(side);
        std::fs::create_dir_all(copy.join("lib")).expect("mkdir");
        std::fs::copy(
            fixture_gem().join("lib/pureleaf.rb"),
            copy.join("lib/pureleaf.rb"),
        )
        .expect("copy the gem source");
        ok(zeo()
            .arg("--package")
            .arg("pureleaf")
            .arg("-o")
            .arg(dir.join(format!("{side}.zeopkg")))
            .arg(copy.join("lib/pureleaf.rb"))
            .env("ZEO_CACHE", "0"));
    }
    let one = std::fs::read(dir.join("one.zeopkg")).expect("read one");
    let two = std::fs::read(dir.join("two.zeopkg")).expect("read two");
    assert!(one == two, "the two builds are byte-identical");
}
