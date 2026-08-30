//! `have_func` answers the same thing under zeo as under ruby.
//!
//! Every C extension branches on these probes to pick between a modern path
//! and a compatibility fallback, so a wrong answer does not fail loudly -- it
//! compiles the wrong half of the gem, and the failure surfaces later as a
//! symbol collision or an `abort` in `extconf.rb`. Three gems in the Gemfile
//! have been broken by a wrong answer here, twice, so the truth table gets a
//! test rather than a comment.
//!
//! zeo answers with a COMPILE where ruby uses a LINK, because zeo's runtime
//! has no shared library to link against (`LIBRUBYARG` is empty), and its
//! headers declare exactly the rows it carries. That trade is only sound with
//! a fallthrough: a system function is declared in a header `ruby.h` never
//! includes, so only the linker can find it. See `parse/shims/mkmf_zeo.rb`.

use std::path::PathBuf;
use std::process::Command;

/// The five shapes `try_func` distinguishes, and what each must answer.
/// A `(probe, expected)` pair per row.
const PROBES: &[(&str, bool)] = &[
    // A system function, bare. Declared in `syslog.h`, which no zeo header
    // includes -- so the compile cannot see it and only the link can. This
    // is the row that cost syslog and fiddle their builds.
    ("have_func('openlog')", true),
    // A system function in its call form, with the header that declares it.
    ("have_func('dlopen(0,0)', 'dlfcn.h')", true),
    // A ruby row zeo carries, bare.
    ("have_func('rb_str_new')", true),
    // A ruby row zeo carries, in the call form `f(args)`. Upstream sends this
    // shape straight to the linker, which is what broke io-console 0.8.2:
    // zeo exports the symbol and still answered no.
    ("have_func('rb_syserr_fail_str(0, Qnil)')", true),
    // A ruby row zeo does not have: declared by no header, exported by no
    // system library. The fallthrough must NOT turn this into a yes.
    ("have_func('rb_zeo_no_such_function')", false),
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/zeo sits two levels under the workspace root")
        .to_path_buf()
}

#[test]
fn every_have_func_shape_answers_what_ruby_answers() {
    let root = repo_root();
    let dir = std::env::temp_dir().join(format!("zeo-mkmf-probes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create the probe dir");

    // `$stdout.sync`, because mkmf redirects stdout into its own log around
    // every probe and this test reads what comes back out -- the redirect
    // being a no-op is its own bug (see `IO#initialize_copy`).
    let mut src = String::from("require \"mkmf\"\n$stdout.sync = true\n");
    for (i, (probe, _)) in PROBES.iter().enumerate() {
        src.push_str(&format!("puts \"row{i}=#{{{probe}}}\"\n"));
    }
    std::fs::write(dir.join("extconf.rb"), src).expect("write extconf.rb");

    let out = Command::new(env!("CARGO_BIN_EXE_zeo"))
        .current_dir(&dir)
        .env_remove("RUBYOPT")
        .env_remove("RUBYLIB")
        .arg("-I")
        .arg(root.join("gems/rubygems/lib"))
        .arg("extconf.rb")
        .output()
        .expect("run extconf.rb under zeo");
    let stdout = String::from_utf8_lossy(&out.stdout);

    let mut wrong = Vec::new();
    for (i, (probe, expected)) in PROBES.iter().enumerate() {
        let line = format!("row{i}=");
        let answer = stdout
            .lines()
            .find_map(|l| l.trim().strip_prefix(&line))
            .map(|v| v == "true");
        match answer {
            Some(got) if got == *expected => {}
            Some(got) => wrong.push(format!("{probe}: answered {got}, ruby answers {expected}")),
            None => wrong.push(format!("{probe}: no answer in the output")),
        }
    }
    assert!(
        wrong.is_empty(),
        "have_func disagrees with ruby:\n  {}\n--- stdout ---\n{stdout}\n--- stderr ---\n{}",
        wrong.join("\n  "),
        String::from_utf8_lossy(&out.stderr),
    );
    let _ = std::fs::remove_dir_all(&dir);
}
