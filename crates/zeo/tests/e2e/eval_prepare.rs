//! `Zeo::Eval.prepare` -- the async snippet compile. A zeo-only surface
//! (CRuby has no `Zeo`), so every expectation here is zeo's own; the
//! executed VALUES still match what CRuby computes for the same source.

use crate::support::{RunResult, run_ruby, run_ruby_configured};

#[test]
fn a_prepared_snippet_compiles_off_thread_and_the_eval_runs_it() {
    let out: RunResult = run_ruby(
        r#"
        src = "40 + 2"
        b = TOPLEVEL_BINDING
        status = Zeo::Eval.prepare(src, b, "bench.rb", 1, true)
        raise "unexpected #{status}" unless [:compiling, :ready].include?(status)
        50.times do
          break if Zeo::Eval.prepare(src, b, "bench.rb", 1, true) == :ready
          sleep 0.02
        end
        raise "never ready" unless Zeo::Eval.prepare(src, b, "bench.rb", 1, true) == :ready
        p eval(src, b, "bench.rb", 1)
        "#,
    );
    assert!(out.status.success(), "stderr: {}", out.stderr);
    assert_eq!(out.stdout, "42\n");
}

#[test]
fn a_refused_snippet_reports_failed_and_the_eval_raises() {
    let out = run_ruby(
        r#"
        src = "1 +"
        b = TOPLEVEL_BINDING
        Zeo::Eval.prepare(src, b, "bad.rb", 1)
        status = nil
        50.times do
          status = Zeo::Eval.prepare(src, b, "bad.rb", 1)
          break if status == :failed
          sleep 0.02
        end
        p status
        begin
          eval(src, b, "bad.rb", 1)
        rescue SyntaxError
          puts "SyntaxError"
        end
        "#,
    );
    assert!(out.status.success(), "stderr: {}", out.stderr);
    assert_eq!(out.stdout, ":failed\nSyntaxError\n");
}

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
    for out in [run_ruby(src), run_ruby_configured(src, &[("ZEO_GVL", "1")], &[])] {
        assert!(out.status.success(), "stderr: {}", out.stderr);
        assert_eq!(out.stdout, "12\n12\n");
    }
}
