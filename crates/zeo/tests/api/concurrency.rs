use crate::support::{run_ruby, run_ruby_configured};

// ---------------------------------------------------------------------------
// Fiber -- corosensei-backed stackful coroutines behind the
// zeo-fiber shim (see that crate's docs for the one quarantined unsafe
// block and its invariants). Every snippet oracle-verified against real
// `ruby` first; error messages are CRuby-verbatim (`cont.c`).
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Threads run as real OS threads -- truly parallel, with no GVL by default;
// `ZEO_GVL=1` serializes them instead (see `zeo_rt::run_main`'s docs).
// `ZEO_THREADS`/`--no-gvl` are accepted but ignored. The real regression
// coverage is every other test in this file; these two only exercise the
// configuration knobs themselves.
// ---------------------------------------------------------------------------

#[test]
fn scheduler_config_knobs_change_nothing_observable() {
    // The same fiber-exercising program (fibers being the most
    // execution-context-sensitive feature) under the default settings, an
    // explicit ZEO_THREADS count, and --no-gvl -- byte-identical output on all
    // three, since the two knobs are ignored.
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

// ---------------------------------------------------------------------------
// Thread/Mutex/Queue over real OS threads (see zeo_rt::thread's docs). These
// tests only assert SYNCHRONIZED, deterministic outcomes. Every snippet
// oracle-verified against real `ruby`; error messages CRuby-verbatim.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Each Ruby Thread is its own OS thread, so the `$!`/HANDLING stack is
// per-thread; Fiber#resume additionally swaps in each fiber's own saved
// stack, giving fibers the isolated execution context CRuby's per-fiber
// `saved_ec` provides. All snippets oracle-verified against real `ruby`.
// ---------------------------------------------------------------------------

#[test]
fn a_thread_blocked_in_accept_does_not_stall_the_connecting_sibling() {
    // The socket-family probe: the acceptor parks in accept(2) BEFORE any
    // client exists (the sleep guarantees it), then main must still be
    // able to ask `server.addr` and connect. This pinned TWO bugs at
    // once: accept holding the listener MUTEX across the blocking wait
    // deadlocked main's `addr` in every mode, and in ZEO_GVL=1 mode the
    // parked holder additionally needed to release the Gvl. The echo
    // round trip also rides the with_file seam for the socket gets/puts.
    let source = r#"
        require "socket"
        server = TCPServer.new("127.0.0.1", 0)
        echo = Thread.new do
          c = server.accept
          c.puts c.gets
          c.close
        end
        sleep 0.2
        s = TCPSocket.new("127.0.0.1", server.addr[1])
        s.puts "ping"
        p s.gets
        echo.join
        s.close
        server.close
        "#;
    for env in [&[][..], &[("ZEO_GVL", "1")][..]] {
        let result = run_ruby_configured(source, env, &[]);
        assert!(result.status.success(), "stderr: {}", result.stderr);
        assert_eq!(result.stdout, "\"ping\\n\"\n", "env: {env:?}");
    }
}

#[test]
fn concurrent_whole_file_reads_and_writes_stay_isolated_per_thread() {
    // The bounded-fs family under thread pressure in both scheduler
    // modes: each thread File.writes its own file then reads it back
    // three ways (read, binread byte count, readlines chomped) while its
    // siblings do the same; Dir's entry scan then sees all three files.
    let source = r#"
        dir = ENV["TMPDIR"] || "/tmp"
        files = 3.times.map { |i| File.join(dir, "zeo_fs_probe_#{Process.pid}_#{i}") }
        results = files.each_with_index.map do |f, i|
          Thread.new do
            File.write(f, "data#{i}\nrow#{i}\n")
            [File.read(f), File.binread(f).bytesize, File.readlines(f, chomp: true)]
          end
        end.map(&:value)
        p results
        stem = "zeo_fs_probe_#{Process.pid}"
        p Dir.entries(dir).count { |n| n.start_with?(stem) }
        files.each { |f| File.delete(f) }
        "#;
    for env in [&[][..], &[("ZEO_GVL", "1")][..]] {
        let result = run_ruby_configured(source, env, &[]);
        assert!(result.status.success(), "stderr: {}", result.stderr);
        assert_eq!(
            result.stdout,
            "[[\"data0\\nrow0\\n\", 11, [\"data0\", \"row0\"]], \
             [\"data1\\nrow1\\n\", 11, [\"data1\", \"row1\"]], \
             [\"data2\\nrow2\\n\", 11, [\"data2\", \"row2\"]]]\n3\n",
            "env: {env:?}"
        );
    }
}

#[test]
fn a_threaded_echo_server_serves_three_concurrent_clients() {
    // The socket family end-to-end under thread pressure in both
    // scheduler modes: one acceptor thread serves three client threads
    // that connect and round-trip concurrently -- accept, connect, and
    // the per-socket gets/puts all park-and-release independently.
    let source = r#"
        require "socket"
        server = TCPServer.new("127.0.0.1", 0)
        port = server.addr[1]
        srv = Thread.new do
          3.times do
            c = server.accept
            c.puts("echo:" + c.gets.chomp)
            c.close
          end
        end
        clients = 3.times.map do |i|
          Thread.new do
            s = TCPSocket.new("127.0.0.1", port)
            s.puts "c#{i}"
            line = s.gets
            s.close
            line
          end
        end
        p clients.map(&:value).sort
        srv.join
        server.close
        "#;
    for env in [&[][..], &[("ZEO_GVL", "1")][..]] {
        let result = run_ruby_configured(source, env, &[]);
        assert!(result.status.success(), "stderr: {}", result.stderr);
        assert_eq!(
            result.stdout, "[\"echo:c0\\n\", \"echo:c1\\n\", \"echo:c2\\n\"]\n",
            "env: {env:?}"
        );
    }
}

#[test]
fn two_threads_parked_in_accept_on_one_server_each_get_a_client() {
    // Two acceptors block on the SAME listener (each accept(2) runs on
    // its own dup(2) with the mutex dropped), then two clients connect;
    // the kernel hands each connection to exactly one parked acceptor.
    let source = r#"
        require "socket"
        server = TCPServer.new("127.0.0.1", 0)
        port = server.addr[1]
        acceptors = 2.times.map do
          Thread.new do
            c = server.accept
            c.puts("hi " + c.gets.chomp)
            c.close
          end
        end
        sleep 0.2
        replies = 2.times.map do |i|
          Thread.new do
            s = TCPSocket.new("127.0.0.1", port)
            s.puts "t#{i}"
            r = s.gets
            s.close
            r
          end
        end.map(&:value)
        p replies.sort
        acceptors.each(&:join)
        server.close
        "#;
    for env in [&[][..], &[("ZEO_GVL", "1")][..]] {
        let result = run_ruby_configured(source, env, &[]);
        assert!(result.status.success(), "stderr: {}", result.stderr);
        assert_eq!(
            result.stdout, "[\"hi t0\\n\", \"hi t1\\n\"]\n",
            "env: {env:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Ractor -- real OS threads sharing the global heap, with the ruby 4.0
// PORT MODEL's frozen-or-copy boundary discipline (see zeo_rt::ractor's
// docs, incl. the documented divergences: an unfrozen Object is rejected
// rather than deep-copied, process-shared globals). Oracle-verified.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// The comprehensive cross-feature sweep. Every scenario was first run as one
// combined program, oracle-verified byte-for-byte against real `ruby`, then
// confirmed byte-identical whether run with the default settings,
// ZEO_THREADS=4, or --no-gvl (all equivalent, since those knobs are ignored).
// Individual tests below keep failures localized; the composite test at the
// end is the headline "the config toggles change nothing observable" check.
// ---------------------------------------------------------------------------

#[test]
fn the_concurrency_composite_is_invariant_across_scheduler_modes() {
    // A well-synchronized program mixing Threads, a Mutex counter, a Queue
    // rendezvous, and parallel Ractors produces byte-identical output with the
    // default settings, an explicit worker count, and --no-gvl.
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
