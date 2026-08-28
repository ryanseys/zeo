//! RubyGems can see the libraries zeo bundles.
//!
//! Everything here was measured broken before `zeo::default_gems` existed,
//! and each row is a separate way the same fiction showed:
//!
//!   Gem.dir                        /usr/local/lib/ruby/gems/4.0.0  (no such dir)
//!   Gem.default_specifications_dir .../specifications/default      (no such dir)
//!   Gem::Specification.stubs.size  0
//!   Gem.loaded_specs               {}
//!   Gem.ruby                       /usr/local/bin/ruby             (no such file)
//!
//! `gem list` printed nothing, `Gem::Specification.find_by_name` raised for
//! every bundled library, and any gem with a C extension died at
//! `extconf failed: No such file or directory`.
//!
//! One test, because it is one expensive compile: a program that requires
//! RubyGems compiles its whole graph, and that is seconds, not milliseconds.
//! Everything cheap enough to unit-test lives in `zeo::default_gems`.

use std::path::PathBuf;
use std::process::Command;

fn zeo_bin() -> PathBuf {
    crate::zeo_bin::zeo_cli().unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn rubygems_sees_the_bundled_libraries_as_default_gems() {
    let prefix =
        std::env::temp_dir().join(format!("zeo-default-gem-store-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&prefix);

    let out = Command::new(zeo_bin())
        .arg("-e")
        .arg(
            r#"require "rubygems"
puts Gem.default_dir
puts Dir.exist?(Gem.default_specifications_dir)
puts Gem::Specification.stubs.size
uri = Gem::Specification.find_by_name("uri")
puts uri.version
puts File.directory?(File.join(uri.full_gem_path, "lib"))
puts uri.require_paths.inspect
"#,
        )
        // A scratch prefix, so the assertions cannot pass on a store some
        // earlier run left in the dev tree's `target/`.
        .env("ZEO_PREFIX", &prefix)
        .env_remove("RUBYOPT")
        .env_remove("RUBYLIB")
        .output()
        .expect("zeo runs");
    assert!(
        out.status.success(),
        "zeo failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 6, "unexpected output:\n{stdout}");

    let store = prefix.join("lib/ruby/gems").join(zeo::default_gems::RUBY_API_VERSION);
    assert_eq!(PathBuf::from(lines[0]), store, "Gem.default_dir");
    assert_eq!(lines[1], "true", "the default-specifications dir exists");
    // A count, not a list: the bundled set changes with every gem added or
    // dropped, and pinning it here would make this test a second inventory to
    // maintain. The floor only has to be far enough above zero -- the bug --
    // to prove the store was really built.
    let stubs: usize = lines[2].parse().expect("a stub count");
    assert!(stubs > 40, "only {stubs} default gems:\n{stdout}");

    // `find_by_name` reaching a library, and its `full_gem_path` resolving
    // through the symlink to the real payload, is the whole round trip.
    assert_eq!(lines[3], "1.1.1", "uri's version");
    assert_eq!(lines[4], "true", "uri's full_gem_path reaches its lib/");
    assert_eq!(lines[5], r#"["lib"]"#, "uri's require_paths");

    let _ = std::fs::remove_dir_all(&prefix);
}
