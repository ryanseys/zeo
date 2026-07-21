use crate::support::{run_ruby, run_ruby_configured};

#[test]
fn fiber_current_storage_kill_and_raise() {
    let result = run_ruby(
        r##"
        p Fiber.current.equal?(Fiber.current)
        Fiber[:level] = "outer"
        seen = []
        f = Fiber.new do
          seen << Fiber[:level]
          Fiber[:level] = "inner"
          seen << Fiber[:level]
          Fiber.yield
        end
        f.resume
        p seen
        p Fiber[:level]
        p Fiber.current.storage
        other = Fiber.new { Fiber.yield }
        other.resume
        begin
          other.storage
        rescue ArgumentError => e
          puts e.message
        end
        k = Fiber.new { Fiber.yield 1; Fiber.yield 2 }
        p k.resume
        p k.kill.equal?(k)
        p k.alive?
        r = Fiber.new do
          begin
            Fiber.yield "one"
          rescue => e
            Fiber.yield "rescued: #{e.message}"
          end
        end
        p r.resume
        p r.raise("boom")
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\n[\"outer\", \"inner\"]\n\"outer\"\n{level: \"outer\"}\n\
         Fiber storage can only be accessed from the Fiber it belongs to\n\
         1\ntrue\nfalse\n\"one\"\n\"rescued: boom\"\n"
    );
}

#[test]
fn thread_current_name_status_and_thread_locals() {
    let result = run_ruby(
        r#"
        p Thread.current == Thread.main
        p Thread.pass
        t = Thread.new { 20 + 22 }
        t.name = "worker"
        p t.name
        p t.value
        p t.status
        p Thread.main.status
        Thread.current[:tag] = "root"
        p Thread.current[:tag]
        p Thread.current.key?(:tag)
        p Thread.current.keys
        Thread.current.thread_variable_set(:count, 3)
        p Thread.current.thread_variable_get(:count)
        p Thread.current.thread_variables
        p Thread.current.report_on_exception
        workers = [Thread.new { 1 }, Thread.new { 2 }]
        workers.each(&:join)
        p workers.map(&:status)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\nnil\n\"worker\"\n42\nfalse\n\"run\"\n\"root\"\ntrue\n[:tag]\n3\n[:count]\ntrue\n[false, false]\n"
    );
}

// ---------------------------------------------------------------------------
// Phase 13.3: Fiber -- corosensei-backed stackful coroutines behind the
// zeo-fiber shim (see that crate's docs for the one quarantined unsafe
// block and its invariants). Every snippet oracle-verified against real
// `ruby` first; error messages are CRuby-verbatim (`cont.c`).
// ---------------------------------------------------------------------------

#[test]
fn fiber_yields_values_in_order_then_returns_the_body_value_and_dies() {
    let result = run_ruby(
        r#"
        f = Fiber.new do
          Fiber.yield 1
          Fiber.yield 2
          3
        end
        puts f.alive?
        puts f.resume
        puts f.resume
        puts f.resume
        puts f.alive?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\n1\n2\n3\nfalse\n");
}

#[test]
fn fiber_resume_and_yield_pass_values_in_both_directions() {
    // resume(v)'s v becomes the suspended Fiber.yield's return value;
    // Fiber.yield(v)'s v becomes resume's return value -- CRuby
    // `make_passing_arg` both ways.
    let result = run_ruby(
        r#"
        g = Fiber.new do |x|
          y = Fiber.yield(x + 1)
          z = Fiber.yield(y + 10)
          z + 100
        end
        puts g.resume(5)
        puts g.resume(6)
        puts g.resume(7)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "6\n16\n107\n");
}

#[test]
fn first_resume_args_bind_to_the_fiber_blocks_params() {
    let result = run_ruby(
        r#"
        h = Fiber.new do |a, b|
          a + b
        end
        puts h.resume(3, 4)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "7\n");
}

#[test]
fn resuming_a_dead_fiber_raises_a_catchable_fiber_error() {
    let result = run_ruby(
        r#"
        d = Fiber.new { :done }
        d.resume
        begin
          d.resume
        rescue FiberError => e
          puts e.send(:message)
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "attempt to resume a terminated fiber\n");
}

#[test]
fn fiber_yield_at_the_root_raises_a_fiber_error() {
    let result = run_ruby(
        r#"
        begin
          Fiber.yield(1)
        rescue FiberError => e
          puts e.send(:message)
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "attempt to yield on a not resumed fiber\n");
}

#[test]
fn an_uncaught_exception_inside_a_fiber_reraises_at_the_resumer_and_kills_the_fiber() {
    let result = run_ruby(
        r#"
        x = Fiber.new do
          raise "boom in fiber"
        end
        begin
          x.resume
        rescue RuntimeError => e
          puts "caught: #{e.send(:message)}"
        end
        puts x.alive?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "caught: boom in fiber\nfalse\n");
}

#[test]
fn nested_fibers_each_yield_to_their_own_resumer() {
    // The exact shape the zeo-fiber TLS save/restore discipline exists
    // for: after the inner fiber yields, the OUTER fiber's own Fiber.yield
    // must suspend the outer one, not touch the inner's suspended yielder.
    // The inner fiber is CREATED outside the outer's block (captured via an
    // ordinary local) because a block literal escaping from inside another
    // escaping block is a PRE-existing Phase 6 scope-cut unrelated to
    // fibers; the nested block-LITERAL form is covered at the Rust level by
    // zeo-fiber's own `nested_fibers_yield_to_their_own_resumers` test.
    let result = run_ruby(
        r#"
        inner = Fiber.new do
          Fiber.yield :from_inner
          :inner_done
        end
        outer = Fiber.new do
          got = inner.resume
          Fiber.yield got
          inner.resume
        end
        puts outer.resume
        puts outer.resume
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "from_inner\ninner_done\n");
}

#[test]
fn a_fiber_body_captures_and_mutates_enclosing_locals() {
    // The fiber's block goes through the ordinary escaping-Proc capture
    // machinery (Phase 6's Arc<Mutex> cells), so shared mutation across
    // suspension points works exactly like any other escaping block.
    let result = run_ruby(
        r#"
        count = 0
        c = Fiber.new do
          count += 10
          Fiber.yield
          count += 100
        end
        c.resume
        puts count
        c.resume
        puts count
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "10\n110\n");
}

// ---------------------------------------------------------------------------
// Phase 13.4: the whole top level runs as `may`'s first coroutine, with the
// worker count as the GVL switch (see `zeo_rt::run_main`'s docs). The
// REAL regression test for this change is every other test in this file --
// all of them now execute through the coroutine-wrapped main. These three
// only cover the configuration knobs themselves.
// ---------------------------------------------------------------------------

#[test]
fn scheduler_config_knobs_change_nothing_observable() {
    // The same fiber-exercising program (fibers being the most
    // execution-context-sensitive feature shipped so far) under the default
    // (workers=1, GVL-emulated), an explicit ZEO_THREADS count, and
    // --no-gvl -- byte-identical output on all three.
    let src = r#"
        f = Fiber.new do |x|
          Fiber.yield(x + 1)
          :done
        end
        puts f.resume(1)
        puts f.resume
        puts f.alive?
    "#;
    let expected = "2\ndone\nfalse\n";
    let default = run_ruby(src);
    assert!(default.status.success(), "stderr: {}", default.stderr);
    assert_eq!(default.stdout, expected);

    let threads4 = run_ruby_configured(src, &[("ZEO_THREADS", "4")], &[]);
    assert!(threads4.status.success(), "stderr: {}", threads4.stderr);
    assert_eq!(threads4.stdout, expected);

    let no_gvl = run_ruby_configured(src, &[], &["--no-gvl"]);
    assert!(no_gvl.status.success(), "stderr: {}", no_gvl.stderr);
    assert_eq!(no_gvl.stdout, expected);
}

#[test]
fn a_malformed_zeo_threads_value_fails_loudly_at_startup() {
    let result = run_ruby_configured("puts 1\n", &[("ZEO_THREADS", "not-a-number")], &[]);
    assert!(!result.status.success());
    assert!(
        result
            .stderr
            .contains("ZEO_THREADS must be a positive integer"),
        "stderr: {}",
        result.stderr
    );
}

// ---------------------------------------------------------------------------
// Phase 13.5: Thread/Mutex/Queue over may's green coroutines (see
// zeo_rt::thread's docs, incl. the documented cooperative-scheduling
// divergence -- these tests only assert SYNCHRONIZED, deterministic
// outcomes). Every snippet oracle-verified against real `ruby`; error
// messages CRuby-verbatim.
// ---------------------------------------------------------------------------

#[test]
fn thread_join_waits_for_the_body_and_returns() {
    // Deterministic under the default single worker: the spawned coroutine
    // first runs when the spawner blocks at join. (Real preemptive ruby
    // agrees on this shape's ordering too -- oracle-verified.)
    let result = run_ruby(
        r#"
        t = Thread.new do
          puts "in thread"
        end
        t.join
        puts "after join"
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "in thread\nafter join\n");
}

#[test]
fn thread_value_returns_the_block_result_and_args_bind_to_params() {
    let result = run_ruby(
        r#"
        v = Thread.new(20, 22) { |a, b| a + b }.value
        puts v
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n");
}

#[test]
fn an_uncaught_exception_in_a_thread_reraises_at_join_and_value() {
    // CRuby stores the exception and re-raises it in whoever joins
    // (`thread.c:1195`). The no-join report_on_exception stderr warning is
    // a documented skip.
    let result = run_ruby(
        r#"
        bad = Thread.new { raise "thread boom" }
        begin
          bad.join
        rescue RuntimeError => e
          puts "joined error: #{e.send(:message)}"
        end
        bad2 = Thread.new { raise "thread boom2" }
        begin
          bad2.value
        rescue RuntimeError => e
          puts "valued error: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "joined error: thread boom\nvalued error: thread boom2\n"
    );
}

#[test]
fn mutex_protected_counter_across_threads_is_exact() {
    // THE canonical threading idiom -- Proc-within-Proc (Thread.new wrapping
    // synchronize), only possible because Phase 13.5 lifted the nested-
    // escaping-block rejection. The sum is deterministic regardless of
    // interleaving; the e2e harness runs this under the default GVL mode,
    // and the same program was manually verified identical under
    // ZEO_THREADS=4 (real parallelism).
    let result = run_ruby(
        r#"
        m = Mutex.new
        count = 0
        t1 = Thread.new { 1000.times { m.synchronize { count += 1 } } }
        t2 = Thread.new { 1000.times { m.synchronize { count += 1 } } }
        t1.join
        t2.join
        puts count
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2000\n");
}

#[test]
fn mutex_error_semantics_match_cruby() {
    let result = run_ruby(
        r#"
        mu = Mutex.new
        begin
          mu.unlock
        rescue ThreadError => e
          puts e.send(:message)
        end
        mu.lock
        puts mu.locked?
        puts mu.owned?
        begin
          mu.lock
        rescue ThreadError => e
          puts e.send(:message)
        end
        mu.unlock
        puts mu.locked?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Attempt to unlock a mutex which is not locked\ntrue\ntrue\ndeadlock; recursive locking\nfalse\n"
    );
}

#[test]
fn mutex_synchronize_returns_the_block_value_and_always_unlocks() {
    let result = run_ruby(
        r#"
        mu = Mutex.new
        r = mu.synchronize { 42 }
        puts r
        puts mu.locked?
        begin
          mu.synchronize { raise "inside" }
        rescue RuntimeError => e
          puts "rescued: #{e.send(:message)}"
        end
        puts mu.locked?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\nfalse\nrescued: inside\nfalse\n");
}

#[test]
fn queue_producer_consumer_rendezvous_with_close() {
    // pop blocks (coroutine-yielding) until a value or closure arrives; a
    // closed empty queue pops nil (`thread_sync.c:1034`).
    let result = run_ruby(
        r#"
        q = Queue.new
        producer = Thread.new do
          q.push 1
          q.push 2
          q.close
        end
        consumer = Thread.new do
          total = 0
          loop do
            v = q.pop
            break if v.nil?
            total += v
          end
          total
        end
        producer.join
        puts consumer.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n");
}

#[test]
fn condition_variable_wait_signal_and_broadcast_coordinate_threads() {
    // signal/broadcast return self; wait releases the mutex, parks
    // (coroutine-yielding) until broadcast, then re-acquires. The handoff is
    // deterministic via the shared `ready` flag under the mutex.
    let result = run_ruby(
        r#"
        cv = ConditionVariable.new
        puts cv.signal.equal?(cv)
        puts cv.broadcast.equal?(cv)
        # a lone wait with a timeout returns (does not hang)
        m0 = Mutex.new
        m0.synchronize { cv.wait(m0, 0.01) }
        puts "timeout-ok"

        mutex = Mutex.new
        ready = false
        worker = Thread.new do
          mutex.synchronize do
            cv.wait(mutex) until ready
          end
          puts "woke"
        end
        mutex.synchronize do
          ready = true
          cv.broadcast
        end
        worker.join
        puts "joined"
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\ntrue\ntimeout-ok\nwoke\njoined\n");
}

#[test]
fn push_to_a_closed_queue_raises_closed_queue_error() {
    let result = run_ruby(
        r#"
        qq = Queue.new
        qq.close
        puts qq.closed?
        begin
          qq.push 1
        rescue ClosedQueueError => e
          puts e.send(:message)
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\nqueue closed\n");
}

#[test]
fn queue_length_shovel_and_empty_predicate() {
    let result = run_ruby(
        r#"
        q3 = Queue.new
        q3 << 5
        q3 << 6
        puts q3.length
        puts q3.empty?
        puts q3.pop
        puts q3.pop
        puts q3.empty?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "2\nfalse\n5\n6\ntrue\n");
}

// ---------------------------------------------------------------------------
// Phase 13.6: the `$!`/HANDLING stack is may COROUTINE-local (not
// thread-local -- multiple Ruby Threads share one OS worker under the GVL
// default), and Fiber#resume swaps in each fiber's own saved stack, giving
// fibers the isolated execution context CRuby's per-fiber `saved_ec`
// provides. All snippets oracle-verified against real `ruby`.
// ---------------------------------------------------------------------------

#[test]
fn each_threads_bare_reraise_sees_its_own_handled_exception() {
    // With a thread_local! HANDLING stack this would cross-contaminate the
    // moment two Threads multiplex onto the one default worker.
    let result = run_ruby(
        r#"
        t1 = Thread.new do
          begin
            raise "from t1"
          rescue RuntimeError => e
            begin
              raise
            rescue RuntimeError => inner
              puts "t1 re-raised: #{inner.send(:message)}"
            end
          end
        end
        t1.join
        t2 = Thread.new do
          begin
            raise "from t2"
          rescue RuntimeError => e
            begin
              raise
            rescue RuntimeError => inner
              puts "t2 re-raised: #{inner.send(:message)}"
            end
          end
        end
        t2.join
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "t1 re-raised: from t1\nt2 re-raised: from t2\n"
    );
}

#[test]
fn a_fiber_does_not_see_its_resumers_currently_handled_exception() {
    // CRuby: the fiber has its own errinfo (fresh, nil), so its bare
    // `raise` builds a fresh empty-message RuntimeError instead of
    // re-raising the resumer's in-flight exception -- `fiber saw: []`.
    let result = run_ruby(
        r#"
        f = Fiber.new do
          begin
            raise
          rescue RuntimeError => e
            "fiber saw: [#{e.send(:message)}]"
          end
        end
        begin
          raise "resumer's exception"
        rescue RuntimeError
          puts f.resume
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "fiber saw: []\n");
}

#[test]
fn a_fibers_rescue_state_survives_suspension_isolated_from_the_resumer() {
    // The fiber suspends MID-rescue; the resumer then handles (and
    // finishes handling) its own exception; on re-entry the fiber's bare
    // re-raise must still see the FIBER's exception -- the save/restore
    // swap around every switch, exercised in both directions.
    let result = run_ruby(
        r#"
        g = Fiber.new do
          begin
            raise "fiber's own"
          rescue RuntimeError
            Fiber.yield :suspended_mid_rescue
            begin
              raise
            rescue RuntimeError => again
              again.send(:message)
            end
          end
        end
        puts g.resume
        begin
          raise "resumer noise"
        rescue RuntimeError
          x = 1
        end
        puts g.resume
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "suspended_mid_rescue\nfiber's own\n");
}

#[test]
fn set_thread_module_and_exception_keyword_accessors() {
    // Set#reset, Thread.list, Module#const_set, and the exception accessors that
    // take keyword/positional data: KeyError(key:/receiver:), SystemExit(status),
    // LocalJumpError#reason, FrozenError#receiver.
    let result = run_ruby(
        r#"
        require "set"
        p Set[1, 2, 3].reset.to_a.sort
        p Thread.list.all? { |t| t.is_a?(Thread) }
        module Box; end
        p Box.const_set(:X, 99)
        p Box::X
        ke = KeyError.new("m", key: :k, receiver: {1 => 2})
        p [ke.key, ke.receiver, ke.message]
        p [SystemExit.new(2).status, SystemExit.new(2).success?, SystemExit.new.success?, SystemExit.new(true, "bye").message]
        s = "x".freeze
        begin; s << "y"; rescue FrozenError => e; p e.receiver; end
        def mm; yield; end
        begin; mm; rescue LocalJumpError => e; p [e.reason, e.exit_value]; end
        begin; {}.fetch(:z); rescue KeyError => e; p e.key; end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "[1, 2, 3]\ntrue\n99\n99\n[:k, {1 => 2}, \"m\"]\n[2, false, true, \"bye\"]\n\"x\"\n\
         [:noreason, nil]\n:z\n"
    );
}

#[test]
fn fiber_transfer_root_and_error_guards() {
    // Batch 13: Fiber#transfer round-tripping through the root fiber (the case
    // that used to segfault), Fiber#[]/#[]= storage, the FiberError guards
    // (yield in a transfer-entered fiber, double resume), and Fiber#kill
    // running ensure blocks.
    let result = run_ruby(
        r#"
        main = Fiber.current
        f = Fiber.new do
          v = main.transfer(42)
          main.transfer(v + 1)
        end
        p f.transfer
        p f.transfer(7)

        Fiber[:tag] = :outer
        g = Fiber.new { Fiber[:tag] }
        p g.resume

        begin
          ft = Fiber.new { |x| Fiber.yield x }
          ft.transfer(1)
        rescue FiberError => e
          puts "transfer-yield: #{e.message}"
        end

        h = Fiber.new do
          begin
            Fiber.yield 1
          ensure
            puts "ensure ran"
          end
        end
        h.resume
        p h.kill.is_a?(Fiber)
        p h.alive?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "42\n8\n:outer\ntransfer-yield: attempt to yield on a not resumed fiber\n\
         ensure ran\ntrue\nfalse\n"
    );
}

#[test]
fn thread_registry_and_kill_raise() {
    // Batch 12: Thread.list (main plus live spawns, joined ones pruned) and
    // Thread.list.include? by identity; Thread#kill unwinding a blocked thread
    // through its ensure; Thread#raise injecting a rescuable exception; #kill
    // returning the thread; #exit/#terminate aliases.
    let result = run_ruby(
        r#"
        Thread.report_on_exception = false
        p Thread.list.size
        p Thread.list.include?(Thread.current)
        ts = (1..3).map { Thread.new { 1 } }
        p Thread.list.size
        ts.each(&:join)
        p Thread.list.size

        q = Queue.new
        log = []
        t = Thread.new do
          begin
            log << :started
            q.pop
            log << :unreached
          ensure
            log << :ensure_ran
          end
        end
        Thread.pass
        t.kill
        t.join
        p log
        p t.alive?

        q2 = Queue.new
        r = Thread.new do
          begin
            q2.pop
            "no"
          rescue => e
            "caught: #{e.message}"
          end
        end
        Thread.pass
        r.raise("boom")
        p r.value

        v = Thread.new { q.pop }
        p v.kill.equal?(v)
        v.join
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "1\ntrue\n4\n1\n[:started, :ensure_ran]\nfalse\n\"caught: boom\"\ntrue\n"
    );
}

#[test]
fn one_threads_bad_dispatch_no_longer_kills_the_other_threads() {
    // THE motivating scenario for this phase: the failure surfaces at the
    // bad thread's own join; the healthy worker completes normally.
    let result = run_ruby(
        r#"
        class Plain
        end
        worker = Thread.new { 21 * 2 }
        bad = Thread.new { Plain.new.send(:missing_in_thread) }
        begin
          bad.join
        rescue NoMethodError => e
          puts "joined the failure"
        end
        puts worker.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "joined the failure\n42\n");
}

// ---------------------------------------------------------------------------
// Phase 13.8: Ractor -- real OS threads sharing the global heap, with the
// frozen-or-copy boundary discipline (see zeo_rt::ractor's docs, incl.
// the documented divergences: no Ractor::RemoteError wrapper, RactorError
// standing in for Ractor::Error, process-shared globals). Oracle-verified.
// ---------------------------------------------------------------------------

#[test]
fn ractor_runs_in_parallel_and_returns_its_value() {
    let result = run_ruby(
        r#"
        r = Ractor.new(20, 22) do |a, b|
          a + b
        end
        puts r.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n");
}

#[test]
fn ractor_message_passing_send_and_receive() {
    let result = run_ruby(
        r#"
        worker = Ractor.new do
          msg = Ractor.receive
          msg * 10
        end
        worker.send(7)
        puts worker.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "70\n");
}

#[test]
fn ractor_shareable_tiering_and_make_shareable() {
    let result = run_ruby(
        r#"
        puts Ractor.shareable?(1)
        puts Ractor.shareable?(:sym)
        puts Ractor.shareable?("mutable")
        frozen_str = "frozen".freeze
        puts Ractor.shareable?(frozen_str)
        arr = [1, 2]
        puts Ractor.shareable?(arr)
        Ractor.make_shareable(arr)
        puts Ractor.shareable?(arr)
        puts arr.frozen?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\ntrue\nfalse\ntrue\nfalse\ntrue\ntrue\n"
    );
}

#[test]
fn an_uncaught_exception_in_a_ractor_reraises_at_value() {
    // Documented divergence: the ORIGINAL exception re-raises directly
    // (like Thread#join), not wrapped in Ractor::RemoteError (which needs
    // nested class names + .cause chaining, both documented gaps).
    let result = run_ruby(
        r#"
        bad = Ractor.new do
          raise "ractor boom"
        end
        begin
          bad.value
        rescue RuntimeError => e
          puts "rescued: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "rescued: ractor boom\n");
}

#[test]
#[should_panic(expected = "can not isolate a Proc because it accesses outer variables (x)")]
fn a_ractor_block_capturing_an_outer_local_is_rejected_at_compile_time() {
    // CRuby raises Ractor::IsolationError at Proc-creation time; the AOT
    // compiler knows the capture set statically and rejects at COMPILE
    // time, with CRuby's own message wording.
    let _ = zeo::compile_to_rust(
        r#"
        x = 5
        Ractor.new { x + 1 }
        "#,
    );
}

#[test]
fn an_unfrozen_object_sent_across_a_ractor_boundary_raises_ractor_error() {
    // Documented narrower-than-CRuby divergence: real Ruby deep-copies an
    // unfrozen object; this runtime has no by-name ivar-setting reflection
    // to rebuild one, so it raises a catchable RactorError instead.
    let result = run_ruby(
        r#"
        class Box
          def initialize(v)
            @v = v
          end
        end
        sink = Ractor.new do
          Ractor.receive
        end
        begin
          sink.send(Box.new(1))
        rescue RactorError => e
          puts "rejected: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert!(
        result
            .stdout
            .starts_with("rejected: an unfrozen Object can't cross a Ractor boundary"),
        "stdout: {}",
        result.stdout
    );
}

#[test]
fn a_deeply_frozen_object_crosses_a_ractor_boundary_by_reference() {
    let result = run_ruby(
        r#"
        class Box
          def initialize(v)
            @v = v
          end
          def v
            @v
          end
        end
        b = Box.new(41)
        Ractor.make_shareable(b)
        puts b.frozen?
        sink = Ractor.new do
          got = Ractor.receive
          got.send(:v)
        end
        sink.send(b)
        puts sink.value + 1
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\n42\n");
}

// ---------------------------------------------------------------------------
// Phase 13.9: the comprehensive cross-feature sweep. Every scenario was
// FIRST run as one combined program, oracle-verified byte-for-byte against
// real `ruby`, then confirmed byte-identical under all three scheduler
// modes (default GVL, ZEO_THREADS=4, --no-gvl) and stable across
// repeated 8-worker runs. Individual tests below keep failures localized;
// the composite mode-invariance test at the end is the plan's headline
// "the toggle changes nothing observable" check.
// ---------------------------------------------------------------------------

#[test]
fn fiber_yield_from_a_deep_method_call_stack() {
    // The zeo-fiber shim's raison d'etre: suspension from inside a chain
    // of ordinary compiled method frames that never saw a yielder.
    let result = run_ruby(
        r#"
        class Chain
          def a; b; end
          def b; c; end
          def c
            Fiber.yield :from_deep
            :done
          end
        end
        ch = Chain.new
        f = Fiber.new { ch.a }
        puts f.resume
        puts f.resume
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "from_deep\ndone\n");
}

#[test]
fn fiber_yield_inside_an_inline_times_block() {
    let result = run_ruby(
        r#"
        f = Fiber.new do
          3.times do |i|
            Fiber.yield i
          end
          :end
        end
        puts f.resume
        puts f.resume
        puts f.resume
        puts f.resume
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "0\n1\n2\nend\n");
}

#[test]
fn fiber_yield_through_a_real_escaping_block() {
    // The hardest suspension shape: Fiber.yield fires inside a real Proc
    // (the block each_twice invokes), unwinding through the Proc's closure
    // frame AND each_twice's own method frame to the resumer.
    let result = run_ruby(
        r#"
        class Iter
          def each_twice
            yield 1
            yield 2
          end
        end
        it = Iter.new
        f = Fiber.new do
          it.each_twice do |n|
            Fiber.yield n
          end
          :done
        end
        puts f.resume
        puts f.resume
        puts f.resume
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\n2\ndone\n");
}

#[test]
fn a_fiber_driven_entirely_inside_a_thread() {
    // The fiber table is per-OS-thread; a fiber created and resumed inside
    // one Thread's coroutine works because both operations run on the same
    // worker.
    let result = run_ruby(
        r#"
        t = Thread.new do
          ff = Fiber.new do |x|
            Fiber.yield x + 1
            99
          end
          a = ff.resume(1)
          b = ff.resume
          a + b
        end
        puts t.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "101\n");
}

#[test]
fn an_exception_from_deep_inside_a_fibers_method_stack_reraises_at_the_resumer() {
    let result = run_ruby(
        r#"
        class Deep
          def go; boom; end
          def boom
            raise "deep boom"
          end
        end
        d = Deep.new
        f = Fiber.new { d.go }
        begin
          f.resume
        rescue RuntimeError => e
          puts "resumer caught: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "resumer caught: deep boom\n");
}

#[test]
fn a_fiber_rebinding_a_captured_local_propagates_to_the_enclosing_scope() {
    // From the reference project's own regression corpus
    // (fiber_reassign_capture.rb): REBINDING (not just mutating) a captured
    // name inside the fiber must write through the shared cell.
    let result = run_ruby(
        r#"
        s = "old"
        f = Fiber.new do
          s = "new"
          Fiber.yield
        end
        f.resume
        puts s
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "new\n");
}

#[test]
fn three_threads_join_out_of_creation_order() {
    let result = run_ruby(
        r#"
        t1 = Thread.new { 1 }
        t2 = Thread.new { 2 }
        t3 = Thread.new { 3 }
        puts t3.value + t1.value + t2.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "6\n");
}

#[test]
fn mutex_synchronize_exits_with_a_break_value_and_unlocks() {
    let result = run_ruby(
        r#"
        mu = Mutex.new
        r = mu.synchronize { break 5 }
        puts r
        puts mu.locked?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "5\nfalse\n");
}

#[test]
fn a_blocked_consumer_is_woken_by_a_producer_thread() {
    // Under the default single worker the consumer runs first (at
    // main's `.value` yield), genuinely BLOCKS on the empty pop
    // (a coroutine-yielding Condvar wait), and is woken by the producer --
    // exercising the real wakeup path, deterministically.
    let result = run_ruby(
        r#"
        q = Queue.new
        consumer = Thread.new { q.pop }
        producer = Thread.new { q.push 42 }
        puts consumer.value
        producer.join
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n");
}

#[test]
fn closing_a_queue_wakes_a_blocked_consumer_with_nil() {
    let result = run_ruby(
        r#"
        q = Queue.new
        consumer = Thread.new do
          v = q.pop
          if v.nil?
            :got_nil
          else
            :got_value
          end
        end
        closer = Thread.new { q.close }
        puts consumer.value
        closer.join
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "got_nil\n");
}

#[test]
fn globals_and_cvars_are_mutex_protectable_across_threads() {
    let result = run_ruby(
        r#"
        $total = 0
        class Counter
          @@n = 0
          def self.bump
            @@n = @@n + 1
          end
          def self.n
            @@n
          end
        end
        m = Mutex.new
        t1 = Thread.new { 100.times { m.synchronize { $total += 1 } } }
        t2 = Thread.new { 100.times { m.synchronize { Counter.bump } } }
        t1.join
        t2.join
        puts $total
        puts Counter.n
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "100\n100\n");
}

#[test]
fn begin_rescue_ensure_and_retry_work_inside_threads() {
    let result = run_ruby(
        r#"
        t1 = Thread.new do
          begin
            raise "in thread"
          rescue RuntimeError => e
            "rescued"
          ensure
            x = 1
          end
        end
        puts t1.value
        t2 = Thread.new do
          attempts = 0
          begin
            attempts += 1
            raise "flaky" if attempts < 3
            attempts
          rescue RuntimeError
            retry
          end
        end
        puts t2.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "rescued\n3\n");
}

#[test]
fn a_thread_and_the_fiber_it_resumes_have_isolated_handling() {
    // The plan's own listed 13.9 scenario: a Thread mid-rescue resumes a
    // fiber whose bare raise must see an EMPTY $!, not the thread's.
    let result = run_ruby(
        r#"
        t = Thread.new do
          f = Fiber.new do
            begin
              raise
            rescue RuntimeError => e
              "fiber saw: [#{e.send(:message)}]"
            end
          end
          begin
            raise "thread's exception"
          rescue RuntimeError
            f.resume
          end
        end
        puts t.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "fiber saw: []\n");
}

#[test]
fn no_method_error_propagates_out_of_fibers_and_ractors() {
    let result = run_ruby(
        r#"
        class Bare; end
        f = Fiber.new { Bare.new.send(:nope) }
        begin
          f.resume
        rescue NoMethodError
          puts "rescued in resumer"
        end
        puts f.alive?
        r = Ractor.new { Bare.new.send(:nope) }
        begin
          r.value
        rescue NoMethodError
          puts "rescued at value"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "rescued in resumer\nfalse\nrescued at value\n"
    );
}

#[test]
fn several_ractors_run_in_parallel_and_all_values_collect() {
    let result = run_ruby(
        r#"
        r1 = Ractor.new(1) { |n| n * 10 }
        r2 = Ractor.new(2) { |n| n * 10 }
        r3 = Ractor.new(3) { |n| n * 10 }
        puts r1.value + r2.value + r3.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "60\n");
}

#[test]
fn ractor_messages_are_received_in_fifo_order() {
    let result = run_ruby(
        r#"
        r = Ractor.new do
          a = Ractor.receive
          b = Ractor.receive
          c = Ractor.receive
          a * 100 + b * 10 + c
        end
        r.send(1)
        r.send(2)
        r.send(3)
        puts r.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "123\n");
}

#[test]
fn a_fiber_works_inside_a_ractor() {
    // The fiber table is thread-pinned and a Ractor is its own OS thread --
    // create and drive entirely within it.
    let result = run_ruby(
        r#"
        r = Ractor.new do
          f = Fiber.new do
            Fiber.yield 1
            2
          end
          a = f.resume
          b = f.resume
          a + b
        end
        puts r.value
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "3\n");
}

#[test]
fn a_frozen_error_raised_inside_a_thread_surfaces_at_join() {
    let result = run_ruby(
        r#"
        a = [1]
        a.freeze
        t = Thread.new { a[0] = 9 }
        begin
          t.join
        rescue FrozenError => e
          puts "at join: #{e.send(:message)}"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "at join: can't modify frozen Array: [1]\n");
}

#[test]
fn freezing_in_one_thread_is_visible_after_join() {
    // Threads share the heap (no Ractor boundary): a freeze in one is the
    // same AtomicBool every other execution context reads.
    let result = run_ruby(
        r#"
        s = "x"
        t = Thread.new { s.freeze }
        t.join
        puts s.frozen?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "true\n");
}

#[test]
fn index_compound_assignment_works_inside_a_thread_block() {
    // From the reference corpus (index_opassign_in_thread_block.rb): the
    // `Seq` desugar's hidden temps declared inside an escaping Proc's own
    // prelude, against a captured-cell array.
    let result = run_ruby(
        r#"
        arr = [10]
        t = Thread.new do
          50.times { arr[0] += 1 }
        end
        t.join
        puts arr[0]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "60\n");
}

#[test]
fn the_concurrency_composite_is_invariant_across_scheduler_modes() {
    // The plan's headline 13.9 check: a well-synchronized program mixing
    // Threads, a Mutex counter, a Queue rendezvous, and parallel Ractors
    // produces byte-identical output under the GVL default, an explicit
    // worker count, and --no-gvl.
    let src = r#"
        m = Mutex.new
        count = 0
        t1 = Thread.new { 200.times { m.synchronize { count += 1 } } }
        t2 = Thread.new { 200.times { m.synchronize { count += 1 } } }
        q = Queue.new
        consumer = Thread.new do
          total = 0
          loop do
            v = q.pop
            break if v.nil?
            total += v
          end
          total
        end
        producer = Thread.new do
          25.times { |i| q.push i }
          q.close
        end
        r1 = Ractor.new(5) { |n| n * n }
        r2 = Ractor.new(6) { |n| n * n }
        t1.join
        t2.join
        producer.join
        puts count
        puts consumer.value
        puts r1.value + r2.value
    "#;
    let expected = "400\n300\n61\n";
    let default = run_ruby(src);
    assert!(default.status.success(), "stderr: {}", default.stderr);
    assert_eq!(default.stdout, expected);
    let threads4 = run_ruby_configured(src, &[("ZEO_THREADS", "4")], &[]);
    assert!(threads4.status.success(), "stderr: {}", threads4.stderr);
    assert_eq!(threads4.stdout, expected);
    let no_gvl = run_ruby_configured(src, &[], &["--no-gvl"]);
    assert!(no_gvl.status.success(), "stderr: {}", no_gvl.stderr);
    assert_eq!(no_gvl.stdout, expected);
}

#[test]
fn a_fiber_reached_through_a_dynamic_receiver_still_resumes() {
    // The static fast path emits `fiber_resume` directly; a fiber held in a
    // collection/ivar dispatches through the runtime's Fiber table instead.
    let result = run_ruby(
        r#"
        holder = [Fiber.new { Fiber.yield 1; 2 }]
        p holder[0].resume
        p holder[0].alive?
        p holder[0].resume
        p holder[0].alive?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "1\ntrue\n2\nfalse\n");
}

#[test]
fn thread_join_and_value_via_dynamic_dispatch() {
    // The threads live in an Array, so `join`/`value` dispatch dynamically
    // (the runtime Thread table), not the static codegen fast path.
    let result = run_ruby(
        r#"
        threads = 3.times.map { |i| Thread.new { i * 10 } }
        threads.each(&:join)
        p threads.map(&:value)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "[0, 10, 20]\n");
}

#[test]
fn queue_and_mutex_methods_on_a_poly_receiver() {
    // A Queue/Mutex held in a collection is a dynamically-typed (Poly)
    // receiver, so these methods dispatch through the runtime Path-2 tables
    // rather than the static Path-1 codegen arm. Both must agree.
    let result = run_ruby(
        r#"
        qs = [Queue.new]
        qs.each do |q|
          q.push(10)
          q << 20
          q.enq(30)
        end
        q = qs.first
        p q.length
        p q.empty?
        p q.pop
        q.close
        p q.closed?
        p q.pop
        p q.pop

        locks = { m: Mutex.new }
        m = locks[:m]
        p m.locked?
        r = m.synchronize do
          p m.owned?
          42
        end
        p r
        p m.locked?
        p q.send(:size)
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "3\nfalse\n10\ntrue\n20\n30\nfalse\ntrue\n42\nfalse\n0\n"
    );
}

#[test]
fn sized_queue_back_pressure_try_lock_and_thread_namespacing() {
    // SizedQueue bounds #push; Mutex#try_lock is non-blocking; and the whole
    // Thread::* family reports its CRuby-faithful qualified name.
    let result = run_ruby(
        r#"
        p Queue
        p SizedQueue
        p Mutex
        q = SizedQueue.new(2)
        p q.class
        p q.is_a?(Queue)
        p q.max
        producer = Thread.new do
          5.times { |i| q.push(i) }
          q.close
        end
        got = []
        while (v = q.pop)
          got << v
        end
        producer.join
        p got
        s = SizedQueue.new(3)
        s << "a" << "b"
        p s.size
        s.max = 5
        p s.max
        m = Mutex.new
        p m.try_lock
        p m.try_lock
        m.unlock
        p m.try_lock
        m.unlock
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Thread::Queue\nThread::SizedQueue\nThread::Mutex\n\
         Thread::SizedQueue\ntrue\n2\n[0, 1, 2, 3, 4]\n2\n5\ntrue\nfalse\ntrue\n"
    );
}

#[test]
fn blockless_thread_and_fiber_raise() {
    // A blockless `Thread.new`/`Fiber.new` raises at runtime in real Ruby
    // (thread.c:1034) rather than being a static error, and both are
    // rescuable -- so the program must compile and run.
    let result = run_ruby(
        r##"
        def err
          yield
        rescue => e
          "#{e.class}: #{e.message}"
        end
        puts err { Thread.new }
        puts err { Fiber.new }
        t = Thread.new { 7 }
        p t.value
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "ThreadError: must be called with a block\n\
         ArgumentError: tried to create Proc object without a block\n\
         7\n",
    );
}
