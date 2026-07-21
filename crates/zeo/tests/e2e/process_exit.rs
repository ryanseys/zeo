use crate::support::run_ruby;

#[test]
fn backtick_interpolates_and_runs_a_shell_command() {
    // Interpolation shares the normal dstr path; a `;` forces the `/bin/sh -c`
    // path (shell metacharacter), while the bare word execs directly.
    let result = run_ruby(
        r#"
        name = "world"
        print `echo hi #{name}`
        print `echo a; echo b`
        puts $?.exited?
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "hi world\na\nb\ntrue\n");
}

#[test]
fn explicit_return_inside_a_block_exits_the_enclosing_method() {
    // `c` is a `New`-assigned LOCAL, not a method parameter -- see
    // `an_escaping_block_can_mutate_an_ivar_via_self_capture`'s comment for
    // why (params are always statically `Poly`, a separate pre-existing
    // gap unrelated to this test's actual point: `return` inside a real
    // escaping Proc unwinding all the way out of `find_even`, not just the
    // block/`.each_num` call).
    let result = run_ruby(
        r#"
        class Collector
          def each_num(a, b, c)
            yield a
            yield b
            yield c
          end
        end
        class Finder
          def find_even
            c = Collector.new
            c.each_num(1, 3, 4) { |n| return n if n % 2 == 0 }
            -1
          end
        end
        puts Finder.new.find_even
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "4\n");
}

#[test]
fn uncaught_raise_with_no_rescue_anywhere_exits_with_the_message() {
    let result = run_ruby("raise \"boom\"\n");
    assert!(!result.status.success());
    assert!(
        result.stderr.contains("uncaught exception: boom"),
        "stderr: {}",
        result.stderr
    );
}

#[test]
fn uncaught_exceptions_still_exit_nonzero_through_the_coroutine_boundary() {
    // The top-level uncaught-raise contract (message on stderr, exit 1)
    // must survive the body now running inside a may coroutine and its
    // Result crossing a join back to the OS main thread.
    let result = run_ruby("raise \"through the boundary\"\n");
    assert!(!result.status.success());
    assert!(
        result
            .stderr
            .contains("uncaught exception: through the boundary"),
        "stderr: {}",
        result.stderr
    );
}

#[test]
fn signal_module_and_signal_exception_surface() {
    // SignalException/Interrupt resolve a signal name<->number (#signo/#signm,
    // arg-form validation) and the Signal module answers list/signame/trap
    // (trap is a validated no-op that records the prior action). Byte-verified
    // against ruby 4.0.5 on darwin.
    let result = run_ruby(
        r#"
        p Interrupt.new.signo
        p Interrupt.new.message
        p Interrupt.new("stop").signm
        e = SignalException.new(9, "custom"); p [e.signo, e.message, e.signm]
        p SignalException.new(9).message
        p SignalException.new("INT").signo
        p SignalException.new(:TERM).message
        p((SignalException.new("KILL", "x") rescue $!.class))
        p((SignalException.new("NOPE") rescue $!.class))
        begin; raise SignalException, "SIGINT"; rescue SignalException => x; p x.signo; end
        p Signal.list["INT"]
        p Signal.list.class
        p Signal.signame(15)
        p Signal.signame(2.9)
        p Signal.signame(999)
        p((Signal.signame(nil) rescue $!.class))
        p Signal.trap("USR1", "IGNORE")
        p Signal.trap("USR1", "DEFAULT")
        p((Signal.trap("KILL", "IGNORE") rescue $!.class))
        p Signal.class
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "2\n\"Interrupt\"\n\"stop\"\n[9, \"custom\", \"custom\"]\n\"SIGKILL\"\n2\n\"SIGTERM\"\n\
         ArgumentError\nArgumentError\n2\n2\nHash\n\"TERM\"\n\"INT\"\nnil\nTypeError\n\
         \"DEFAULT\"\n\"IGNORE\"\nErrno::EINVAL\nModule\n"
    );
}

#[test]
fn env_enumerable_and_mutator_surface() {
    // ENV's read-only surface (value?/each_value/min via the Hash snapshot) and
    // the mutators (update/merge!/reject!/delete_if). Uses unique names/values
    // so it never depends on the ambient environment.
    let result = run_ruby(
        r#"
        ENV["ZZE_A"] = "uniqA"; ENV["ZZE_B"] = "uniqB"
        p ENV.value?("uniqA")
        p ENV.has_value?("nope_zzz_xyz")
        p ENV.update("ZZE_A" => "uniq9")["ZZE_A"]
        p ENV.merge!("ZZE_C" => "uniq3")["ZZE_C"]
        p ENV.reject! { |k, v| false }
        p [ENV.each_value.is_a?(Enumerator), ENV.count { |k, v| k.start_with?("ZZE_") } >= 3]
        ENV.delete_if { |k, v| k.start_with?("ZZE_") }
        p ENV.key?("ZZE_C")
        p ENV.to_s
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\nfalse\n\"uniq9\"\n\"uniq3\"\nnil\n[true, true]\nfalse\n\"ENV\"\n"
    );
}

#[test]
fn an_uncaught_no_method_error_exits_via_the_ordinary_top_level_handler() {
    let result = run_ruby(
        r#"
        class Plain
        end
        Plain.new.send(:missing)
        "#,
    );
    assert!(!result.status.success());
    assert!(
        result
            .stderr
            .contains("uncaught exception: undefined method 'missing'"),
        "stderr: {}",
        result.stderr
    );
}

/// `ENV` is an Object with Hash-shaped methods, NOT a Hash (`ENV.class` is
/// `Object` -- oracle-verified), and it reads the LIVE environment.
#[test]
fn env_is_an_object_with_hash_shaped_methods() {
    let result = run_ruby(
        r#"
        p ENV.class
        ENV["ZEO_E2E"] = "set"
        p ENV["ZEO_E2E"]
        p ENV.key?("ZEO_E2E")
        p ENV.fetch("ZEO_E2E")
        p ENV.fetch("ZEO_E2E_ABSENT", "default")
        p ENV.fetch("ZEO_E2E_ABSENT") { |k| "computed:#{k}" }
        p ENV["ZEO_E2E_ABSENT"]
        p ENV.delete("ZEO_E2E")
        p ENV.key?("ZEO_E2E")
        p ENV.delete("ZEO_E2E")
        p ENV.to_h.class
        p ENV.keys.class
        ENV["ZEO_E2E2"] = "x"
        ENV["ZEO_E2E2"] = nil
        p ENV["ZEO_E2E2"]
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Object\n\"set\"\ntrue\n\"set\"\n\"default\"\n\"computed:ZEO_E2E_ABSENT\"\nnil\n\"set\"\nfalse\nnil\nHash\nArray\nnil\n"
    );
}

/// ENV's error shapes: a non-String key is a TypeError, a bare `fetch` miss
/// is a KeyError.
#[test]
fn env_error_shapes() {
    let result = run_ruby(
        r#"
        begin
          ENV[:PATH]
        rescue TypeError => e
          puts "TypeError"
        end
        begin
          ENV.fetch("ZEO_DEFINITELY_ABSENT")
        rescue KeyError => e
          puts "KeyError"
        end
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "TypeError\nKeyError\n");
}

#[test]
fn nonterminating_and_aborting_operations_raise() {
    // Guards for operations that would otherwise hang or kill the process.
    // A zero step never advances, so CRuby rejects it up front
    // (numeric.c:2888) -- via a Ruby-level `==`, so `0.0` trips it too.
    // Materializing an endless range would grow a vector until the process
    // died (range.c:1023). `String#*` past a sane cap would hand the
    // allocator an impossible request, which ABORTS rather than raising;
    // zeo caps it and raises a catchable ArgumentError, where CRuby --
    // whose guard covers only the length multiplication -- reaches the
    // allocator and raises NoMemoryError. That last one is a deliberate,
    // documented divergence toward a rescuable failure.
    let result = run_ruby(
        r##"
        def err
          yield
        rescue => e
          "#{e.class}: #{e.message}"
        end
        puts err { 1.step(10, 0) { } }
        puts err { 1.step(10, 0.0) { } }
        puts err { Rational(1, 2).step(Rational(5, 2), 0) { } }
        puts err { (1..).to_a }
        puts err { (1..).entries }
        p (1..4).to_a
        puts err { "x" * -1 }
        puts err { "x" * (1 << 60) }
        p "ab" * 3
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "ArgumentError: step can't be 0\n\
         ArgumentError: step can't be 0\n\
         ArgumentError: step can't be 0\n\
         RangeError: cannot convert endless range to an array\n\
         RangeError: cannot convert endless range to an array\n\
         [1, 2, 3, 4]\n\
         ArgumentError: negative argument\n\
         ArgumentError: string size too big\n\
         \"ababab\"\n",
    );
}

#[test]
fn process_times_returns_a_process_tms_struct() {
    // Process.times returns a Process::Tms with Float utime/stime/cutime/cstime
    // (via getrusage); the class name and struct-style inspect match CRuby.
    let result = run_ruby(
        r##"
        p Process::Tms
        t = Process.times
        p t.class
        p t.utime.class
        p t.stime >= 0.0
        p t.cutime.class
        p t.cstime.class
        p t.inspect.start_with?("#<struct Process::Tms utime=")
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "Process::Tms\nProcess::Tms\nFloat\ntrue\nFloat\nFloat\ntrue\n"
    );
}
