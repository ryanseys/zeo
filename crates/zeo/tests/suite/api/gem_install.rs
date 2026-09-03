//! `zeo gem build` then `zeo gem install --local`, with a C extension.
//!
//! The whole install pipeline in one test, and every step of it was broken at
//! some point: package a gem, unpack it, run its `extconf.rb`, build its C,
//! and load the result.
//!
//! The unpack is why the assertions compare BYTES rather than just checking
//! that files appeared. `Gem::Package` extracts with
//! `IO.copy_stream(tar.io, out, entry.size)`, and `copy_stream` dropped its
//! length -- so every file came out as its own text followed by the rest of
//! the archive. Ruby never reads past the code it needs, so a pure-ruby gem
//! installed and worked; a C extension compiled megabytes of trailing NUL
//! bytes, and a gem install looked like a hang.
//!
//! No network: the gem is built here and installed with `--local`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn zeo() -> PathBuf {
    crate::zeo_bin::zeo_cli().unwrap_or_else(|e| panic!("{e}"))
}

fn have(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn write(dir: &Path, rel: &str, content: &str) -> PathBuf {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("create parent");
    std::fs::write(&path, content).expect("write source");
    path
}

/// A gem whose C extension defines one method, plus the files an install has
/// to reproduce byte for byte.
fn stage_gem(dir: &Path) {
    write(
        dir,
        "zeoprobe.gemspec",
        r#"Gem::Specification.new do |s|
  s.name = "zeoprobe"
  s.version = "0.1.0"
  s.summary = "a C extension for zeo's install tests"
  s.authors = ["zeo"]
  s.files = ["lib/zeoprobe.rb", "ext/zeoprobe/extconf.rb", "ext/zeoprobe/zeoprobe.c", "data.bin"]
  s.extensions = ["ext/zeoprobe/extconf.rb"]
  s.require_paths = ["lib"]
end
"#,
    );
    write(
        dir,
        "lib/zeoprobe.rb",
        "require \"zeoprobe/zeoprobe\"\nmodule ZeoProbe\n  VERSION = \"0.1.0\"\nend\n",
    );
    write(
        dir,
        "ext/zeoprobe/extconf.rb",
        "require \"mkmf\"\ncreate_makefile(\"zeoprobe/zeoprobe\")\n",
    );
    write(
        dir,
        "ext/zeoprobe/zeoprobe.c",
        r#"#include <ruby.h>

static VALUE probe_double(VALUE self, VALUE n) {
  return INT2NUM(NUM2INT(n) * 2);
}

void Init_zeoprobe(void) {
  VALUE mod = rb_define_module("ZeoProbe");
  rb_define_singleton_method(mod, "double", probe_double, 1);
}
"#,
    );
    // A file with no trailing newline and a NUL in the middle: the unpack bug
    // showed as extra bytes, so an entry whose exact length matters is the
    // sharpest thing to compare against.
    std::fs::write(dir.join("data.bin"), b"head\x00tail").expect("write data.bin");
}

fn run(zeo: &Path, dir: &Path, store: &Path, args: &[&str]) -> std::process::Output {
    Command::new(zeo)
        .arg("gem")
        .args(args)
        .current_dir(dir)
        .env("GEM_HOME", store)
        .env("GEM_PATH", store)
        .env_remove("RUBYOPT")
        .env_remove("RUBYLIB")
        .output()
        .unwrap_or_else(|e| panic!("spawning zeo gem {args:?}: {e}"))
}

/// IGNORED until zeo's psych grows `Psych::Visitors`. `gem build` writes the
/// gemspec as YAML through `Gem::NoAliasYAMLTree`, which
/// `rubygems/psych_tree.rb` only defines when `Psych::Visitors` is there, so
/// the first step of this pipeline raises `uninitialized constant
/// Gem::NoAliasYAMLTree`. Every other step below is known to work.
#[test]
#[ignore = "gem build needs Psych::Visitors"]
fn a_gem_with_a_c_extension_packages_installs_and_loads() {
    if !have("cc") {
        eprintln!("skipping: this machine has no `cc`");
        return;
    }
    let zeo = zeo();
    let dir = std::env::temp_dir().join(format!("zeo-gem-install-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (src, store) = (dir.join("src"), dir.join("store"));
    std::fs::create_dir_all(&src).expect("a source dir");
    std::fs::create_dir_all(&store).expect("a store dir");
    stage_gem(&src);

    let built = run(&zeo, &src, &store, &["build", "zeoprobe.gemspec"]);
    let gem = src.join("zeoprobe-0.1.0.gem");
    assert!(
        built.status.success() && gem.is_file(),
        "gem build failed:\n{}{}",
        String::from_utf8_lossy(&built.stdout),
        String::from_utf8_lossy(&built.stderr)
    );

    let installed = run(
        &zeo,
        &src,
        &store,
        &[
            "install",
            "--local",
            "--no-document",
            gem.to_str().expect("utf-8 path"),
        ],
    );
    assert!(
        installed.status.success(),
        "gem install failed:\n{}{}",
        String::from_utf8_lossy(&installed.stdout),
        String::from_utf8_lossy(&installed.stderr)
    );

    // THE unpack assertion. Every file has to come back exactly as it went
    // in -- not "starts with", which the dropped-length bug also satisfied.
    let unpacked = store.join("gems/zeoprobe-0.1.0");
    for rel in [
        "lib/zeoprobe.rb",
        "ext/zeoprobe/extconf.rb",
        "ext/zeoprobe/zeoprobe.c",
        "data.bin",
    ] {
        let want = std::fs::read(src.join(rel)).expect("the staged file");
        let got = std::fs::read(unpacked.join(rel))
            .unwrap_or_else(|e| panic!("reading the installed {rel}: {e}"));
        assert_eq!(
            got.len(),
            want.len(),
            "{rel} was installed at {} bytes instead of {}",
            got.len(),
            want.len()
        );
        assert_eq!(got, want, "{rel} does not match what was packaged");
    }

    // The extension built and the stamp says so.
    let dlext = if cfg!(target_vendor = "apple") {
        "bundle"
    } else {
        "so"
    };
    let mut products = Vec::new();
    collect(&store.join("extensions"), &mut products);
    let want_so = format!("zeoprobe.{dlext}");
    assert!(
        products.iter().any(|p| p.ends_with(&want_so)),
        "no zeoprobe.{dlext} under the extensions dir, only {products:?}\n{}",
        String::from_utf8_lossy(&installed.stdout)
    );
    assert!(
        products.iter().any(|p| p.ends_with("gem.build_complete")),
        "the install left no gem.build_complete"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

fn collect(dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else {
            out.push(path.to_string_lossy().into_owned());
        }
    }
}
