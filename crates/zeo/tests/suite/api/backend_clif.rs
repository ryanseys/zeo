//! `zeo backend`: a binary from CLIF text, with nothing of Ruby read.
//!
//! Two roads prove it. A program written BY HAND in the subset a front end
//! outside this crate would emit links and runs -- that is the
//! reader, the name resolution and the sidecar with no Rust lowering in
//! the loop. And the Rust emitter's own `--emit-clif --emit-zeodata` text
//! rebuilds into a binary that prints what `zeo build` prints -- so every
//! function the emitter writes is text the backend reads back.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn zeo() -> PathBuf {
    crate::zeo_bin::zeo_cli().unwrap_or_else(|e| panic!("{e}"))
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("zeo-backend-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

fn run(cmd: &mut Command) -> Output {
    let out = cmd.output().expect("the command runs");
    assert!(
        out.status.success(),
        "{cmd:?} failed:\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

/// A built program's stdout, run under the leak check so an ownership slip
/// in hand-written text fails here rather than in a later program.
fn stdout_of(bin: &Path) -> String {
    let out = run(Command::new(bin).env("ZEO_RT_LEAKCHECK", "1"));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// `puts 1 + 2`, in the outlined-frame shape a front end writes first:
/// the frame's enter and pop as calls, the two literals as tagged slots,
/// `sadd_overflow` with the runtime's slow path on the overflow arm, and
/// the pool bracket around the statement. The rodata holds the file name
/// and the frame label the sidecar beside it carries.
const ADD_CLIF: &str = r#"
;; the top level of `puts 1 + 2`
function %zeo_toplevel(i64) -> i32 system_v {
    ss0 = explicit_slot 24, align = 8
    ss1 = explicit_slot 24, align = 8
    ss2 = explicit_slot 24, align = 8
    ss3 = explicit_slot 24, align = 8
    gv0 = symbol colocated %zeo_rodata
    sig0 = (i64, i64, i64, i64, i32, i32) -> i32 system_v
    sig1 = () -> i64 system_v
    sig2 = (i32) system_v
    sig3 = (i64, i64, i64) system_v
    sig4 = (i64) system_v
    sig5 = (i64, i64, i64) -> i32 system_v
    sig6 = () system_v
    fn0 = %zeo_rt_frame_enter sig0
    fn1 = %zeo_rt_pool_mark sig1
    fn2 = %zeo_rt_set_line sig2
    fn3 = %zeo_rt_int_add_slow sig3
    fn4 = %zeo_rt_pool_push sig4
    fn5 = %zeo_rt_kernel_puts sig5
    fn6 = %zeo_rt_pool_reset sig4
    fn7 = %zeo_rt_frame_pop sig6

block0(v0: i64):
    v1 = symbol_value.i64 gv0
    v2 = iconst.i64 6
    v3 = iadd v1, v2
    v4 = iconst.i32 0
    v5 = call fn0(v1, v2, v3, v2, v4, v4)   ; "add.rb", "<main>"
    brif v5, block1, block2

block2:
    v6 = call fn1()
    v7 = iconst.i32 1
    call fn2(v7)
    v8 = stack_addr.i64 ss0
    v9 = iconst.i8 2                        ; the Int tag
    v10 = iconst.i64 1
    store notrap aligned v9, v8
    store notrap aligned v10, v8+8
    v11 = stack_addr.i64 ss1
    v12 = iconst.i64 2
    store notrap aligned v9, v11
    store notrap aligned v12, v11+8
    v13 = stack_addr.i64 ss2
    v14, v15 = sadd_overflow v10, v12
    brif v15, block3, block4

block3:
    call fn3(v8, v11, v13)                  ; overflowed: a Bignum answers
    jump block5

block4:
    store notrap aligned v9, v13
    store notrap aligned v14, v13+8
    jump block5

block5:
    v16 = load.i8 notrap aligned v13
    v17 = iconst.i8 16
    v18 = icmp uge v16, v17
    brif v18, block6, block7

block6:
    call fn4(v13)                           ; a heap answer is the pool's
    jump block7

block7:
    v19 = stack_addr.i64 ss3
    v20 = iconst.i64 1
    v21 = call fn5(v13, v20, v19)
    brif v21, block1, block8

block8:
    call fn6(v6)
    v22 = iconst.i64 0
    store notrap aligned v22, v0
    store notrap aligned v22, v0+8
    store notrap aligned v22, v0+16
    v23 = iconst.i32 0
    jump block9(v23)

block1:
    v24 = iconst.i32 1
    jump block9(v24)

block9(v25: i32):
    call fn7()
    return v25
}
"#;

/// `"add.rb<main>"`.
const ADD_ZEODATA: &str = r#"{"abi_version": 1, "rodata": "6164642e72623c6d61696e3e"}"#;

#[test]
fn a_hand_written_program_links_and_runs() {
    crate::paths::runtime_archive().unwrap_or_else(|e| panic!("{e}"));
    let dir = scratch("hand");
    std::fs::write(dir.join("add.clif"), ADD_CLIF).expect("write the text");
    std::fs::write(dir.join("add.zeodata"), ADD_ZEODATA).expect("write the sidecar");
    let bin = dir.join("add");
    run(Command::new(zeo())
        .arg("backend")
        .arg(dir.join("add.clif"))
        .arg("-o")
        .arg(&bin));
    assert_eq!(stdout_of(&bin), "3\n");
}

/// The emitter's own text, for a program with a `def`: the sidecar carries
/// the symbol pool, the call sites, the method row and its reflection.
#[test]
fn the_emitters_text_rebuilds_into_the_same_program() {
    crate::paths::runtime_archive().unwrap_or_else(|e| panic!("{e}"));
    let dir = scratch("emitted");
    let source = "def fib(n)\n  if n < 2\n    n\n  else\n    fib(n - 1) + fib(n - 2)\n  end\nend\n\
                  puts fib(10)\np method(:fib).arity\np method(:fib).parameters\n";
    let rb = dir.join("fib.rb");
    std::fs::write(&rb, source).expect("write the program");
    let built = dir.join("built");
    run(Command::new(zeo())
        .arg("build")
        .arg(&rb)
        .arg("-o")
        .arg(&built));
    run(Command::new(zeo())
        .arg(format!("--emit-clif={}", dir.join("fib.clif").display()))
        .arg(format!(
            "--emit-zeodata={}",
            dir.join("fib.zeodata").display()
        ))
        .arg(&rb));
    let rebuilt = dir.join("rebuilt");
    run(Command::new(zeo())
        .arg("backend")
        .arg(dir.join("fib.clif"))
        .arg("--data")
        .arg(dir.join("fib.zeodata"))
        .arg("-o")
        .arg(&rebuilt));
    let want = stdout_of(&built);
    assert_eq!(want, "55\n1\n[[:req, :n]]\n");
    assert_eq!(stdout_of(&rebuilt), want);
}

/// A program past the subset has no sidecar, and the refusal names the
/// shape rather than writing a description that would boot wrong.
#[test]
fn a_program_the_sidecar_cannot_describe_is_refused_by_name() {
    let dir = scratch("refused");
    let rb = dir.join("klass.rb");
    std::fs::write(&rb, "class K\n  def v = 1\nend\nputs K.new.v\n").expect("write");
    let out = Command::new(zeo())
        .arg(format!("--emit-clif={}", dir.join("klass.clif").display()))
        .arg(format!(
            "--emit-zeodata={}",
            dir.join("klass.zeodata").display()
        ))
        .arg(&rb)
        .output()
        .expect("zeo runs");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--emit-zeodata: the program has a class"),
        "{stderr}"
    );
    assert!(!dir.join("klass.zeodata").exists());
}

/// A superclass only the backend can judge: it owns the builtin ids, so a
/// front end names its superclass and the answer arrives here. Any builtin
/// CLASS is registered -- the registrar the shape needs follows from the
/// ancestors -- and a builtin MODULE is refused with the name in the
/// message, because a class cannot inherit one.
#[test]
fn a_builtin_superclass_is_a_class_or_a_refusal() {
    let dir = scratch("superclass");
    // The empty top level: `<main>` answers nil with no statement of its
    // own, so the class row is the only thing the backend has to resolve.
    const EMPTY: &str = "\
function %zeo_toplevel(i64) -> i32 system_v {
block0(v0: i64):
    v1 = iconst.i64 0
    store notrap aligned v1, v0
    store notrap aligned v1, v0+8
    store notrap aligned v1, v0+16
    v2 = iconst.i32 0
    return v2
}
";
    let sidecar = |superclass: &str| {
        format!(
            r#"{{"abi_version":1,"toplevel":"zeo_toplevel","classes":[{{"name":"Boom","superclass":"{superclass}","kind":"class","ivars":[]}}]}}"#
        )
    };
    for (superclass, ok) in [
        ("StandardError", true),
        ("String", true),
        ("Integer", true),
        ("Comparable", false),
    ] {
        let clif = dir.join(format!("{superclass}.clif"));
        let data = dir.join(format!("{superclass}.zeodata"));
        std::fs::write(&clif, EMPTY).expect("write");
        std::fs::write(&data, sidecar(superclass)).expect("write");
        let out = Command::new(zeo())
            .arg("backend")
            .arg(&clif)
            .arg("-o")
            .arg(dir.join(superclass))
            .output()
            .expect("zeo runs");
        assert_eq!(out.status.success(), ok, "{superclass}");
        if !ok {
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(
                stderr.contains(&format!("`Boom` names the superclass `{superclass}`")),
                "{stderr}"
            );
        }
    }
}
