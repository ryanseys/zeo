//! The official C-gem sweep: every gem in `Gemfile.lock` that ships a C
//! extension is built FROM ITS OWN SOURCE through zeo's C-API route and run
//! against the answer ruby 4.0.6 recorded for the same program.
//!
//! This is the C-API's health gate. `docs/LIMITATIONS.md` names it; until
//! this file existed nothing performed it, and the API's only runtime
//! evidence was 69 lines of purpose-built test C.
//!
//! Per gem: `crates/zeo/tests/fixtures/capi_sweep/<gem>/smoke.rb` is a short
//! program hitting the gem's C entry points, and `smoke.expected` beside it
//! is ruby's stdout, recorded by `cargo xtask capi-sweep bless <gem>` under
//! the same `Gemfile.lock`. zeo runs the program with its own copy of the
//! library RETIRED (`ZEO_DISABLE_BUILTIN`) and the store gem resolving, so
//! the `require` reaches the gem's `extconf.rb`, `cc`, `dlopen` and `Init_`.
//!
//! A pass needs three things at once: exit 0, byte-identical stdout, and
//! the runtime's own "a C extension loaded" sentinel on stderr -- without
//! the third a green row could be zeo's Rust half answering, which is the
//! trap `ZEO_DISABLE_BUILTIN` was built to expose.
//!
//! `XFAIL.json` is the ledger of gems that do not pass yet, each with the
//! reason. An XFAIL that passes FAILS its test ("stale XFAIL"), so the
//! ledger only ever shrinks by a commit that says why.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    crate::paths::workspace_root()
}

fn fixtures() -> PathBuf {
    root().join("crates/zeo/tests/fixtures/capi_sweep")
}

/// The gems the sweep covers, in the lock's spelling.
const SWEPT: &[&str] = &[
    "bigdecimal",
    "date",
    "debug",
    "digest",
    "erb",
    "fiddle",
    "io-console",
    "json",
    "nkf",
    "openssl",
    "prism",
    "psych",
    "rbs",
    "stringio",
    "strscan",
    "syslog",
    "zlib",
];

/// Locked gems with an `ext/` that the sweep deliberately leaves out, each
/// with its reason. A new C gem in the lock that is on neither list fails
/// `every_locked_c_gem_is_swept_or_excluded`.
const EXCLUDED: &[(&str, &str)] = &[
    (
        "racc",
        "zeo declines the C accelerator by design (tests/divergences/racc_declines_the_c_accelerator.rb)",
    ),
    (
        "resolv",
        "its only extension is ext/win32, a Windows-only build",
    ),
];

fn xfail() -> BTreeMap<String, String> {
    let path = fixtures().join("XFAIL.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn sweep(gem: &str) {
    let dir = fixtures().join(gem);
    let smoke = dir.join("smoke.rb");
    let expected = std::fs::read(dir.join("smoke.expected")).unwrap_or_else(|_| {
        panic!(
            "{} has no recorded answer; run `cargo xtask capi-sweep bless {gem}`",
            dir.display()
        )
    });
    let store = zeo::bundled::store_dir(&root())
        .expect("vendor/bundle holds the lock's gems (run `make deps`)");
    let out = Command::new(crate::zeo_bin::zeo_cli().unwrap_or_else(|e| panic!("{e}")))
        .arg("--bundle-gemfile")
        .arg(root().join("Gemfile"))
        .arg("--gem-path")
        .arg(&store)
        .arg(&smoke)
        .current_dir(root())
        .env("ZEO_DISABLE_BUILTIN", gem)
        .env("ZEO_CACHE", "0")
        // Only the arming line is wanted; a full debug log is megabytes.
        .env("ZEO_LOG", "zeo_rt::gvl=debug")
        .env_remove("RUBYOPT")
        .env_remove("RUBYLIB")
        .output()
        .expect("zeo runs");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let loaded = stderr.contains(zeo_rt::gvl::CEXT_ARMED_SENTINEL);
    let same = out.stdout == expected;
    let passed = out.status.success() && same && loaded;
    let report = || {
        format!(
            "gem: {gem}\nexit: {}\nstdout matches ruby: {same}\nC extension loaded: {loaded}\n\
             --- zeo stdout ---\n{}\n--- expected ---\n{}\n--- stderr (tail) ---\n{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&expected),
            stderr
                .lines()
                .rev()
                .take(25)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n"),
        )
    };
    match (xfail().get(gem), passed) {
        (None, true) => {}
        (None, false) => panic!("{}", report()),
        (Some(why), false) => eprintln!("XFAIL {gem}: {why}"),
        (Some(why), true) => {
            panic!("stale XFAIL: {gem} passes now; delete its row from XFAIL.json (it said: {why})")
        }
    }
}

macro_rules! swept {
    ($($name:ident => $gem:literal),* $(,)?) => {
        $(
            #[test]
            fn $name() {
                sweep($gem)
            }
        )*
    };
}

swept! {
    bigdecimal => "bigdecimal",
    date => "date",
    debug => "debug",
    digest => "digest",
    erb => "erb",
    fiddle => "fiddle",
    io_console => "io-console",
    json => "json",
    nkf => "nkf",
    openssl => "openssl",
    prism => "prism",
    psych => "psych",
    rbs => "rbs",
    stringio => "stringio",
    strscan => "strscan",
    syslog => "syslog",
    zlib => "zlib",
}

/// The ledger names only gems the sweep runs -- a typo in a key would
/// otherwise excuse nothing and be noticed by nobody.
#[test]
fn the_xfail_ledger_names_only_swept_gems() {
    let stray: Vec<String> = xfail()
        .into_iter()
        .filter(|(gem, _)| !SWEPT.contains(&gem.as_str()))
        .map(|(gem, why)| format!("{gem}: {why}"))
        .collect();
    assert!(
        stray.is_empty(),
        "XFAIL.json names gems the sweep does not run:\n{}",
        stray.join("\n")
    );
}

/// Every locked gem that carries an `extconf.rb` is either swept or
/// excluded with a reason, so a new C gem entering the lock is a decision
/// rather than a gap.
#[test]
fn every_locked_c_gem_is_swept_or_excluded() {
    let store = zeo::bundled::store_dir(&root())
        .expect("vendor/bundle holds the lock's gems (run `make deps`)");
    let mut missing = Vec::new();
    for lib in zeo::bundled::resolved_libraries(&root()) {
        if !lib.dir.starts_with(&store) || !lib.dir.join("ext").is_dir() {
            continue;
        }
        let known =
            SWEPT.contains(&lib.name.as_str()) || EXCLUDED.iter().any(|(g, _)| *g == lib.name);
        if !known {
            missing.push(lib.name);
        }
    }
    assert!(
        missing.is_empty(),
        "locked C gems on neither list (add a smoke, or exclude with a reason): {missing:?}"
    );
}
