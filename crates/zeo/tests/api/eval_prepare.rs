//! `Zeo::Eval.prepare` -- the async snippet compile. A zeo-only surface
//! (CRuby has no `Zeo`), so every expectation here is zeo's own; the
//! executed VALUES still match what CRuby computes for the same source.

use crate::support::{run_ruby, run_ruby_configured};

#[test]
fn exiting_while_a_worker_compiles_is_clean() {
    // A snippet big enough that the worker is still mid-compile at exit;
    // the detached thread must neither hang the exit tail nor corrupt it.
    let big = "x = 0\n".to_string() + &"x += 1\n".repeat(2000);
    let src = format!(
        r#"
        Zeo::Eval.prepare({big:?}, TOPLEVEL_BINDING, "big.rb", 1)
        puts "bye"
        "#
    );
    let out = run_ruby(&src);
    assert!(out.status.success(), "stderr: {}", out.stderr);
    assert_eq!(out.stdout, "bye\n");
    assert_eq!(out.stderr, "");
}

#[test]
fn prepare_racing_a_sync_eval_agrees_in_both_scheduler_modes() {
    let src = r#"
        s = "[1, 2, 3].map { |x| x * 2 }.sum"
        t = Thread.new { Zeo::Eval.prepare(s, TOPLEVEL_BINDING, "r.rb", 1, true) }
        p eval(s, TOPLEVEL_BINDING, "r.rb", 1)
        t.join
        p eval(s, TOPLEVEL_BINDING, "r.rb", 1)
    "#;
    for out in [
        run_ruby(src),
        run_ruby_configured(src, &[("ZEO_GVL", "1")], &[]),
    ] {
        assert!(out.status.success(), "stderr: {}", out.stderr);
        assert_eq!(out.stdout, "12\n12\n");
    }
}
