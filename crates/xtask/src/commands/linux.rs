//! Run zeo's suites on Linux, in the container the repo-root `Dockerfile`
//! builds. The stand-in for a CI leg, and the only place the Linux link path
//! is exercised at all.
//!
//! Every stage tees its output to `target/linux-logs/<stage>.log`. The
//! container runs `--rm`, so its own logs die with it -- a run whose output
//! only existed there could not be read back.
//!
//! Every stage needs `build` to have run first: nothing rebuilds the `zeo`
//! BINARY for a test target. The target volume persists between runs, so a
//! second `build` is incremental.
//!
//! The container runs as ROOT, and one golden can tell:
//! `test/compiler/builtins/process_identity_rows.rb` expects `Errno::EPERM`
//! from `Sys.setuid("root")`
//! and its siblings, which succeed here. CI and any developer machine run
//! unprivileged, so the expectation is right and this leg is the odd one out
//! -- do not bless it away.

use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;

use crate::{Error, root, root_join};

const DEFAULT_STAGES: &[&str] = &["build", "test", "units", "natlibs", "valgrind"];
const EXTRA_STAGES: &[&str] = &["gate", "cross", "dist", "shell", "all"];

const USAGE: &str = "\
usage: cargo xtask linux <stage>... | -E '<nextest expr>'

stages:
  build     cargo build --workspace
  test      the dev loop: the corpus on the JIT plus the AOT smoke tier
  gate      -P full: every leg over the whole corpus (release boundaries)
  units     zeo + zeo-rt + zeo-capi unit suites
  natlibs   diff link.rs's glibc table against rustc
  valgrind  leak-check a linked program
  cross     x86_64 compile check
  dist      build a linux release tarball into target/dist
  shell     an interactive prompt in the image
  all       build test units natlibs valgrind

-E '<expr>' runs one nextest filter, for triage after a red run.
";

pub fn run(args: &[String]) -> Result<(), Error> {
    let mut filter = None;
    let mut wanted: Vec<String> = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "-E" => {
                filter = Some(
                    rest.next()
                        .cloned()
                        .ok_or_else(|| Error::new(format!("-E wants an expression\n\n{USAGE}")))?,
                );
            }
            "--help" | "-h" => {
                print!("{USAGE}");
                return Ok(());
            }
            other if other.starts_with('-') => {
                return Err(Error::new(format!("unknown option {other:?}\n\n{USAGE}")));
            }
            other => wanted.push(other.to_string()),
        }
    }

    if let Some(filter) = filter {
        return stage("-E", &nextest_filter(&filter));
    }
    if wanted.is_empty() || wanted == ["all"] {
        wanted = DEFAULT_STAGES.iter().map(|s| s.to_string()).collect();
    }
    if let Some(unknown) = wanted
        .iter()
        .find(|s| !DEFAULT_STAGES.contains(&s.as_str()) && !EXTRA_STAGES.contains(&s.as_str()))
    {
        return Err(Error::new(format!("unknown stage: {unknown}")));
    }
    for name in &wanted {
        stage(name, &script_for(name)?)?;
    }
    Ok(())
}

/// RELEASE, not dev. Every AOT golden LINKS a whole binary against
/// `libzeo.a`, and the dev archive is 315 MB against release's 87 MB --
/// measured, a hello compile+link is 1.48s dev and 0.69s release, and this
/// leg pays that 4,061 times. `ZEO_CLIF_VERIFY=1` keeps what
/// `debug_assertions` was buying here: the Cranelift verifier and the
/// ownership ledger.
fn profile() -> String {
    env_or("ZEO_LINUX_PROFILE", "release")
}

fn cargo_profile() -> &'static str {
    if profile() == "release" {
        "--release"
    } else {
        ""
    }
}

/// cargo's dev profile writes to `debug/`, which is why the two names cannot
/// be one value.
fn target_dir() -> &'static str {
    if profile() == "release" {
        "release"
    } else {
        "debug"
    }
}

/// The VM's own width. The e2e tier LINKS a whole binary per test, so this run
/// is bound by `cc` far more than by the compiler -- threads are the lever
/// that matters, and the ceiling is the podman machine's cpu count
/// (`podman machine set --cpus N`, which needs the machine stopped).
fn threads() -> String {
    env_or("ZEO_LINUX_THREADS", "8")
}

fn env_or(key: &str, fallback: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| fallback.into())
}

/// Under `target/`, which is already ignored -- a verification log is build
/// output, not something to keep.
fn logs_dir() -> PathBuf {
    root_join("target/linux-logs")
}

fn nextest_filter(expr: &str) -> String {
    format!(
        "cargo nextest run {} -p zeo --test-threads {} --no-fail-fast -E '{expr}'",
        cargo_profile(),
        threads()
    )
}

fn script_for(name: &str) -> Result<String, Error> {
    let profile = cargo_profile();
    let threads = threads();
    Ok(match name {
        // `deps` first: the gem store the compiler resolves its bundled
        // libraries out of. It is the same store on either platform -- source
        // gems, nothing built -- so the mounted `/src/vendor` the host filled
        // is already complete and this says nothing.
        "build" => format!("cargo xtask deps && cargo build --workspace {profile}"),
        // The default nextest profile: every corpus suite on the JIT, plus
        // the AOT smoke tier really linked. Each case spawns a child zeo and
        // the harness caps its memory.
        "test" => {
            format!("cargo nextest run {profile} -p zeo --test-threads {threads} --no-fail-fast")
        }
        // Every leg over the whole corpus: AOT, memcheck, the differentials
        // and the whole-graph milestones. The container loop's dominant cost,
        // so it is a release-boundary stage rather than a default one.
        "gate" => format!(
            "cargo nextest run {profile} -P full -p zeo \
             --test-threads {threads} --no-fail-fast"
        ),
        // `zeo-capi` last and alone: its dev-dependency turns on zeo-rt's
        // `unit-tables` feature, and cargo unifies features per invocation --
        // shared with the `zeo` tests, the binary the corpus runs would carry
        // every builtin table and stop noticing a dropped one.
        "units" => format!(
            "cargo nextest run {profile} -p zeo -p zeo-rt --test-threads {threads} && \
             cargo nextest run {profile} -p zeo-capi --test-threads {threads}"
        ),
        // The one test that asks rustc for the live answer instead of trusting
        // the table; out of the default profile because it compiles the lib in
        // a probe target dir. This is the platform whose table was never
        // confirmed.
        "natlibs" => format!(
            "cargo nextest run {profile} -P full -p zeo \
             --no-capture -E 'test(natlibs_table_matches_rustc)'"
        ),
        "valgrind" => valgrind_script(),
        "cross" => CROSS_SCRIPT.into(),
        // A NATIVE linux build of the release tarball, from a mac. zeo does
        // not cross-compile to linux; this is the supported path, and the
        // artifact is a genuinely natively-built one, so a cross-compile
        // divergence is structurally impossible.
        "dist" => "cargo xtask dist".into(),
        "shell" => "exec bash".into(),
        other => return Err(Error::new(format!("unknown stage: {other}"))),
    })
}

fn valgrind_script() -> String {
    format!(
        r#"set -e
cd /tmp && printf "%s\n" "class P; def initialize(n) = @n = n; def to_s = \"P(#{{@n}})\"; end" \
  "10.times {{ |i| puts P.new(i) }}" "a = (1..50).map {{ |i| i * i }}; puts a.sum" \
  "begin; raise ArgumentError, \"x\"; rescue => e; puts e.message; end" > vg.rb
/target/{}/zeo -o vg vg.rb
valgrind --error-exitcode=9 --leak-check=full --errors-for-leak-kinds=definite ./vg
"#,
        target_dir()
    )
}

const CROSS_SCRIPT: &str = "\
rustup target add x86_64-unknown-linux-gnu
CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc \\
CC_x86_64_unknown_linux_gnu=x86_64-linux-gnu-gcc \\
AR_x86_64_unknown_linux_gnu=x86_64-linux-gnu-ar \\
cargo check --workspace --target x86_64-unknown-linux-gnu
";

fn stage(name: &str, script: &str) -> Result<(), Error> {
    let tag: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "_.-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    std::fs::create_dir_all(logs_dir())
        .map_err(|e| Error::new(format!("creating {}: {e}", logs_dir().display())))?;
    println!("=== linux: {name}  (log: target/linux-logs/{tag}.log)");
    let code = run_in_container(script, &tag)?;
    if code != 0 {
        eprintln!("linux: {tag} exited {code}");
        return Err(Error::reported());
    }
    Ok(())
}

/// `--platform`: see the Dockerfile header. `--memory`: the golden harness's
/// own RSS watchdog assumes room to work.
///
/// The repo mounts READ-WRITE, and that is deliberate rather than lazy: a
/// golden runs with its cwd set to `tests/`, and a dozen of them create a temp
/// file there on purpose -- the oracle did exactly that when the `.expected`
/// was blessed. A read-only mount turns every one of those into `Errno::EROFS`,
/// which reads as a zeo bug and is not one.
///
/// `--pids-limit`: podman defaults to 2048, and the thread goldens spawn
/// enough OS threads at eight-way parallelism to hit it -- `pthread_create`
/// then fails EAGAIN and the runtime panics mid-test. Another failure that is
/// the harness, not the program.
///
/// `-it` only when there IS a terminal: the same command runs from a non-tty
/// caller (CI), where podman refuses the flag.
fn run_in_container(script: &str, tag: &str) -> Result<i32, Error> {
    let engine = env_or("ZEO_CONTAINER_ENGINE", "podman");
    let mut argv = vec![engine.clone(), "run".into(), "--rm".into()];
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        argv.push("-it".into());
    }
    let logs = logs_dir();
    argv.extend(
        [
            "--platform",
            "linux/arm64",
            "--memory",
            &env_or("ZEO_LINUX_MEMORY", "10g"),
            "-e",
            "ZEO_CLIF_VERIFY=1",
            "--pids-limit",
            "16384",
            "-v",
            &format!("{}:/src", root().display()),
            "-v",
            &format!("{}:/target", env_or("ZEO_LINUX_VOLUME", "zeo-linux-target")),
            "-v",
            &format!("{}:/logs", logs.display()),
            "-w",
            "/src",
            &env_or("ZEO_LINUX_IMAGE", "zeo-linux"),
            // `bash -c`, never `-lc`: a login shell re-reads the image's
            // profile and loses the environment set on the run.
            "bash",
            "-c",
            script,
        ]
        .map(str::to_string),
    );
    tee(&argv, &logs.join(format!("{tag}.log")))
}

/// Streams to the terminal AND the log. The container is `--rm`, so its own
/// output is the only copy.
fn tee(argv: &[String], log: &std::path::Path) -> Result<i32, Error> {
    let mut file = std::fs::File::create(log)
        .map_err(|e| Error::new(format!("creating {}: {e}", log.display())))?;
    // One pipe for both streams, so the log keeps the interleaving the
    // terminal shows rather than two separately-buffered halves.
    let (reader, writer) =
        std::io::pipe().map_err(|e| Error::new(format!("creating a pipe: {e}")))?;
    let err = writer
        .try_clone()
        .map_err(|e| Error::new(format!("cloning the pipe: {e}")))?;
    let mut child = std::process::Command::new(&argv[0])
        .args(&argv[1..])
        .current_dir(root())
        .stdout(writer)
        .stderr(err)
        .spawn()
        .map_err(|e| Error::new(format!("spawning {}: {e}", argv[0])))?;

    let mut lines = std::io::BufReader::new(reader);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match lines.read_until(b'\n', &mut buf) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => return Err(Error::new(format!("reading from the container: {e}"))),
        }
        let mut stdout = std::io::stdout();
        let _ = stdout.write_all(&buf);
        let _ = stdout.flush();
        file.write_all(&buf)
            .map_err(|e| Error::new(format!("writing {}: {e}", log.display())))?;
    }
    let status = child
        .wait()
        .map_err(|e| Error::new(format!("waiting for {}: {e}", argv[0])))?;
    Ok(status.code().unwrap_or(-1))
}
