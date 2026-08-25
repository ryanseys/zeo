use crate::support::{compile_project, run_ruby, run_ruby_project};

#[test]
fn load_reexecutes_every_time_with_fresh_locals_and_never_registers_the_feature() {
    // Three executions: two `load`s plus a `require` -- load never adds to
    // the feature table in real Ruby, so the require still fires. `ticks`
    // restarts at 0 each execution (fresh local scope per load), which the
    // per-splice-instance gensym reproduces.
    let result = run_ruby_project(
        &[
            (
                "tick.rb",
                r#"
                    ticks = 0
                    ticks += 1
                    $total = ($total || 0) + ticks
                    puts "tick! total=#{$total}"
                "#,
            ),
            (
                "main.rb",
                r#"
                    load "./tick.rb"
                    load "./tick.rb"
                    require_relative "tick"
                    puts "end total=#{$total}"
                "#,
            ),
        ],
        "main.rb",
        &[],
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "tick! total=1\ntick! total=2\ntick! total=3\nend total=3\n"
    );
}

#[test]
fn load_cycles_are_detected_at_compile_time() {
    // Real Ruby would recurse forever at runtime (load has no dedup); a
    // compile-time resolver rejects the cycle loudly instead.
    let err = compile_project(
        &[
            ("selfload.rb", "load \"./selfload.rb\"\n"),
            ("main.rb", "load \"./selfload.rb\"\n"),
        ],
        "main.rb",
        &[],
    )
    .unwrap_err();
    assert!(err.contains("cycle"), "unexpected error: {err}");
}

/// A fixed instant read in UTC -- zone-independent, so these are safe to
/// assert on any machine. Oracle-read for epoch 1700000000.
#[test]
fn time_utc_civil_fields() {
    let result = run_ruby(
        r#"
        t = Time.at(1700000000).getutc
        puts t.to_s
        p [t.year, t.month, t.day, t.hour, t.min, t.sec]
        p [t.wday, t.yday]
        p t.to_i
        p t.utc?
        p t.zone
        p t.utc_offset
        p [t.monday?, t.tuesday?]
        puts Time.at(0).getutc.to_s
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "2023-11-14 22:13:20 UTC\n[2023, 11, 14, 22, 13, 20]\n[2, 318]\n1700000000\ntrue\n\"UTC\"\n0\n[false, true]\n1970-01-01 00:00:00 UTC\n"
    );
}

/// `utc`/`gmtime`/`localtime` convert the receiver IN PLACE and answer self;
/// the `get*` forms answer a copy and leave the receiver alone.
#[test]
fn time_mutating_converters_versus_their_copies() {
    let result = run_ruby(
        r#"
        t = Time.at(1700000000)
        u = t.getutc
        p u.utc?
        p t.utc?          # getutc did NOT mutate t
        t.utc
        p t.utc?          # ...but utc did
        puts t.to_s
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "true\nfalse\ntrue\n2023-11-14 22:13:20 UTC\n"
    );
}

#[test]
fn kernel_catch_throw_sleep_via_dynamic_dispatch() {
    // `send`/`&:` force dynamic dispatch through the Kernel table rather than
    // the static codegen fast path.
    let result = run_ruby(
        r#"
        p send(:catch, :done) { throw :done, 42 }
        p [1, 2, 3].map { |x| catch(:skip) { throw :skip, -1 if x == 2; x } }
        send(:sleep, 0)
        puts "slept"
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "42\n[1, -1, 3]\nslept\n");
}

/// `Time#isdst`/`#dst?` report the broken-down time's DST flag; a UTC time is
/// never in DST.
#[test]
fn time_reports_its_dst_flag() {
    let result = run_ruby("t = Time.at(0).utc\np t.isdst\np t.dst?\n");
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "false\nfalse\n");
}

#[test]
fn time_at_honors_the_in_keyword_offset() {
    // `Time.at(epoch, in: offset)` attaches a display utc_offset to the
    // absolute instant (no shift, unlike Time.new's local components).
    let result = run_ruby(
        r#"
        p Time.at(0, in: "+09:00").utc_offset
        p Time.at(0, in: "-05:00").utc_offset
        p Time.at(0, in: 3600).utc_offset
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "32400\n-18000\n3600\n");
}

#[test]
fn time_utc_and_local_accept_a_fractional_seconds_field() {
    // A Rational/Float seconds field splits into the integer second plus an
    // EXACT sub-second (Time stores a rational epoch, so #subsec is exact).
    let result = run_ruby(
        r#"
        p Time.utc(2020, 1, 1, 0, 0, Rational(3, 2)).subsec
        p Time.utc(2020, 1, 1, 0, 0, Rational(3, 2)).sec
        p Time.utc(2020, 1, 1, 0, 0, 2.5).subsec
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(result.stdout, "(1/2)\n1\n(1/2)\n");
}

#[test]
fn time_at_time_ten_arg_and_string_constructors() {
    // Time.at(Time) copies the instant; the 10-arg to_a order; the string
    // form (offset / UTC / local / fractional); and UTC vs a numeric +00:00
    // (which render differently and disagree on #utc?).
    let result = run_ruby(
        r#"
        p Time.at(Time.at(55)).to_i
        p Time.at(Time.at(1.5)).nsec
        p Time.utc(1, 15, 20, 1, 1, 2000, 0, 0, 0, 0).inspect
        t = Time.new("2021-12-25 10:00:00 +09:00")
        p t.utc_offset
        p t.inspect
        u = Time.new("2021-12-25 10:00:00 UTC")
        p u.utc?
        p u.inspect
        f = Time.new("2021-12-25 10:00:00.5 +00:00")
        p f.nsec
        p f.utc?
        p f.inspect
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "55\n500000000\n\"2000-01-01 20:15:01 UTC\"\n32400\n\
         \"2021-12-25 10:00:00 +0900\"\ntrue\n\"2021-12-25 10:00:00 UTC\"\n\
         500000000\nfalse\n\"2021-12-25 10:00:00.5 +0000\"\n"
    );
}

#[test]
fn time_new_rejects_unparseable_strings() {
    let result = run_ruby(
        r##"
        [["garbage", "can't parse"], ["2021-12-25", "no time information"]].each do |s, _|
          begin
            Time.new(s)
          rescue ArgumentError => e
            puts "#{e.class}: #{e.message}"
          end
        end
        "##,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "ArgumentError: can't parse: \"garbage\"\nArgumentError: no time information\n"
    );
}

#[test]
fn time_civil_field_range_validation() {
    // Out-of-range civil fields raise ArgumentError; in-range values that
    // overflow (Feb 30, the 23:59:60 leap second) roll forward instead.
    let result = run_ruby(
        r#"
        def c; begin; yield; rescue ArgumentError; "ArgumentError"; end; end
        p c { Time.utc(2020, 13, 1) }
        p c { Time.utc(2020, 1, 32) }
        p c { Time.utc(2020, 1, 1, 25) }
        p c { Time.utc(2020, 1, 1, 23, 60) }
        p Time.utc(2020, 2, 30).month
        p Time.utc(2020, 12, 31, 23, 59, 60).year
        "#,
    );
    assert!(result.status.success(), "stderr: {}", result.stderr);
    assert_eq!(
        result.stdout,
        "\"ArgumentError\"\n\"ArgumentError\"\n\"ArgumentError\"\n\"ArgumentError\"\n3\n2021\n",
    );
}
