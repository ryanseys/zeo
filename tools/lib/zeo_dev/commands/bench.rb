# frozen_string_literal: true

require "fileutils"
require "tmpdir"

module ZeoDev
  module Commands
    # The performance governor for the structural work.
    #
    # Compiles every `bench/bm_*.rb` with `zeo -o` (so generated programs link
    # the RELEASE runtime, the shipped configuration), verifies its stdout
    # against the committed `.expected` -- a wrong answer fails the run
    # outright, because timing a wrong answer is meaningless -- then times the
    # binary and compares against `bench/baseline.tsv`.
    #
    # Timing discipline: `--runs N` (default 3) executions, keeping the
    # MINIMUM wall time, the standard best-of-N convention for wall-clock
    # benches, since noise on a quiet machine is strictly additive.
    #
    # Interruption-safe: `--update-baseline` REWRITES `baseline.tsv` after
    # every completed benchmark, not once at the end, so a stopped run keeps
    # everything it finished; `--resume` then skips what is already in the
    # file. Because it rewrites, `--filter` has to be accounted for: a
    # filtered update starts from the existing baseline and upserts the rows
    # it measured -- "re-measure X and bank it", not "the baseline is X now".
    #
    # `--compile` measures the other side: what the COMPILER costs and
    # produces per program, rather than how fast the result runs.
    class Bench < Cli
      # Repeat budget: every benchmark gets its first (correctness-gated)
      # run; repeats happen only while total time spent on THAT benchmark is
      # under the budget. Fast benches keep full best-of-N noise rejection; a
      # minutes-long bench times once instead of tripling the suite's wall
      # time, its longer runtime having already averaged out scheduler noise.
      REPEAT_BUDGET_SECS = 30.0

      # The fixed program set `--compile` measures: names paired with
      # repo-relative paths, spanning hello-world to bundler scale. `hello` is
      # synthesized so the smallest possible program is always in the record.
      COMPILE_PROGRAMS = [
        ["hello", nil],
        ["bm_fib", "bench/bm_fib.rb"],
        ["bm_template", "bench/bm_template.rb"],
        ["bm_json_parse", "bench/bm_json_parse.rb"],
        ["bm_micro_lisp", "bench/bm_micro_lisp.rb"],
        ["core_classes", "tests/core_classes.rb"],
        ["uri_parse_and_build", "tests/uri_parse_and_build.rb"],
        ["optparse_subset", "tests/optparse_subset.rb"],
        ["gem_rubygems", "tests/bench/rubygems.rb"],
        ["gem_bundler", "tests/bench/bundler.rb"]
      ].freeze

      BASELINE = "bench/baseline.tsv"
      HISTORY = "bench/history.tsv"
      RUBY_TIMES = "bench/ruby.tsv"
      COMPILE_BASELINE = "bench/compile-baseline.tsv"

      def self.summary = "the runtime performance suite under bench/"

      def self.banner
        "usage: zeo-dev bench [--filter <substr>] [--runs N] [--update-baseline] " \
          "[--resume] [--ruby] [--compile]"
      end

      def defaults = { filter: nil, runs: 3, update: false, resume: false, ruby: false, compile: false }

      def options(o)
        o.on("--filter SUBSTR", "only benchmarks whose name contains SUBSTR") { |v| opts[:filter] = v }
        o.on("--runs N", Integer, "executions per benchmark (default 3)") { |v| opts[:runs] = v }
        o.on("--update-baseline", "bank the measured rows into the baseline") { opts[:update] = true }
        o.on("--resume", "skip benchmarks already in the baseline") { opts[:resume] = true }
        o.on("--ruby", "re-time the CRuby oracle and refresh bench/ruby.tsv") { opts[:ruby] = true }
        o.on("--compile", "measure the COMPILER instead of the generated program") do
          opts[:compile] = true
        end
      end

      def run
        expect_no_args!
        raise Error, "bench: --runs takes a positive integer" unless opts[:runs].positive?
        raise Error, "bench: --resume only makes sense with --update-baseline" if opts[:resume] && !opts[:update]

        # A debug zeo drags per-bench compile time, and `-o` selects the
        # release runtime for the GENERATED programs, which is what we time.
        @zeo = Ruby.build_zeo!
        opts[:compile] ? run_compile : run_runtime
      end

      private

      def path(rel) = File.join(ROOT, rel)

      def programs
        Dir.glob(path("bench/*.rb")).sort
      end

      def selected(name) = opts[:filter].nil? || name.include?(opts[:filter])

      # A progress line the caller sees IMMEDIATELY, even piped or
      # backgrounded: stdout through a pipe is block-buffered.
      def progress(line)
        puts line
        $stdout.flush
      end

      # ---- the runtime suite -------------------------------------------

      def run_runtime
        baseline = read_pairs(path(BASELINE))
        history = read_history
        ruby_times = read_pairs(path(RUBY_TIMES))

        # With --resume, rows already recorded are carried forward and their
        # benchmarks skipped. A FILTERED update carries them forward too, for
        # a different reason: the filter skips every benchmark it does not
        # select, so starting empty would write a baseline containing only the
        # filtered rows and silently discard every other measurement.
        recorded = opts[:resume] || opts[:filter] ? baseline.dup : {}
        results = {}
        failures = []

        programs.each do |rb|
          name = File.basename(rb, ".rb")
          next unless selected(name)

          if opts[:resume] && recorded.key?(name)
            progress(format("%-28s (resumed from baseline)", name))
            next
          end

          secs = begin
            time_program(rb, name)
          rescue Error => e
            progress(format("%-28s FAIL  %s", name, e.message))
            failures << name
            next
          end

          window = bank_history(history, name, secs)
          refresh_ruby_time(ruby_times, name, rb) if opts[:ruby]

          progress(format("%-28s %8.3fs  med5 %7.3fs  %-24s %s",
                          name, secs, median(window.map(&:last)),
                          delta(baseline[name], secs), vs_ruby(ruby_times[name], secs)))
          results[name] = secs

          next unless opts[:update]

          # Upsert, not append: a filtered run starts from the whole
          # baseline, so the row being re-measured is already there.
          recorded[name] = secs
          write_pairs(path(BASELINE), "# bench/baseline.tsv -- zeo-dev bench --update-baseline", recorded)
        end

        report(results, recorded, baseline, failures)
      end

      # Append to the benchmark's rolling window, cap it at 5, and write the
      # file so an interrupted run keeps what it measured. Answers the window.
      def bank_history(history, name, secs)
        window = (history[name] ||= [])
        window << [Time.now.to_i, secs]
        window.shift while window.size > 5
        write_history(history)
        window
      end

      def refresh_ruby_time(ruby_times, name, rb)
        ruby_times[name] = time_oracle(rb)
        write_pairs(path(RUBY_TIMES),
                    "# bench/ruby.tsv -- oracle `ruby` wall time; refresh with --ruby",
                    ruby_times)
      rescue Error => e
        progress(format("%-28s (ruby timing failed: %s)", name, e.message))
      end

      def report(results, recorded, baseline, failures)
        unless failures.empty?
          puts "\n#{failures.size} benchmark(s) failed: #{failures.join(", ")}"
          return 1
        end
        if results.empty? && recorded.empty?
          puts "no benchmarks matched"
          return 1
        end
        # Geometric mean of per-bench ratios -- the one aggregate that treats
        # a 2x win on a fast bench and a 2x loss on a slow one symmetrically.
        ratios = results.filter_map { |n, s| baseline[n] && s / baseline[n] }
        unless ratios.empty?
          geo = Math.exp(ratios.sum { |r| Math.log(r) } / ratios.size)
          puts format("\ngeomean vs baseline: %+.1f%% over %d benchmark(s)",
                      (geo - 1.0) * 100.0, ratios.size)
        end
        puts "baseline updated: #{path(BASELINE)}" if opts[:update]
        0
      end

      def delta(base, secs)
        base ? format("%+6.1f%% vs %.3fs", (secs / base - 1.0) * 100.0, base) : "(no baseline)"
      end

      def vs_ruby(rsecs, secs)
        return "(no ruby ref; record with --ruby)" if rsecs.nil?

        rsecs >= secs ? format("ruby %.3fs (%.1fx faster)", rsecs, rsecs / secs)
                      : format("ruby %.3fs (%.1fx SLOWER)", rsecs, secs / rsecs)
      end

      # Compile, verify against `.expected`, and time the minimum of `runs`.
      def time_program(rb, name)
        # Bytes, not text: some benchmarks print binary output (bm_ao_render
        # emits a PPM image), which is not valid UTF-8.
        expected = read_expected(rb)
        bin = File.join(Dir.tmpdir, "zeo-bench-#{name}")
        compile = Exec.run([@zeo, rb, "-o", bin, "-W0"])
        raise Error, "zeo failed: #{compile.stderr.lines.first&.strip}" unless compile.success?

        best_of(runs_budget) do |i|
          res = Exec.run([bin], capture_stdout: true)
          raise Error, "exited #{res.code.inspect}" unless res.success?
          # Correctness gate on the first run only; repeats time a
          # known-good binary without re-diffing identical output.
          raise Error, "output mismatch vs .expected" if i.zero? && res.stdout != expected

          res
        end
      ensure
        FileUtils.rm_f(bin) if bin
      end

      # The real `ruby` on this benchmark, same discipline: output verified on
      # the first run (an oracle mismatch means the snapshot is stale, so fail
      # loudly), best-of-runs under the same repeat budget.
      def time_oracle(rb)
        expected = read_expected(rb)
        best_of(runs_budget) do |i|
          res = Exec.run([Ruby.oracle, rb], capture_stdout: true)
          raise Error, "#{Ruby.oracle} exited #{res.code.inspect}: #{res.stderr.strip}" unless res.success?
          raise Error, "ruby output mismatch vs .expected (stale snapshot?)" if i.zero? && res.stdout != expected

          res
        end
      end

      def read_expected(rb)
        p = "#{rb}.expected"
        raise Error, "reading #{p}: not found" unless File.exist?(p)

        File.binread(p)
      end

      def runs_budget = opts[:runs]

      # Best-of-N wall time, stopping early once the repeat budget is spent.
      def best_of(runs)
        spent = 0.0
        best = nil
        runs.times do |i|
          break if i.positive? && spent >= REPEAT_BUDGET_SECS

          t0 = Process.clock_gettime(Process::CLOCK_MONOTONIC)
          yield i
          secs = Process.clock_gettime(Process::CLOCK_MONOTONIC) - t0
          spent += secs
          best = best.nil? ? secs : [best, secs].min
        end
        best
      end

      # ---- the compile side --------------------------------------------

      # What the compiler costs and produces per program. Folded in from the
      # deleted `compile-bench`, minus the three columns that measured the
      # removed rustc emitter.
      #
      # `frontend_ms` is best-of-N `--emit-clif` wall time, the renderer the
      # BUILD path uses. `peak_rss` takes the WORST run rather than the best:
      # time is best-of-N because the fastest run is the least disturbed, but
      # memory is a ceiling question, and the most a compile ever held is the
      # number that decides whether it fits.
      def run_compile
        rows = {}
        COMPILE_PROGRAMS.each do |name, rel|
          next unless selected(name)

          rb = rel ? path(rel) : write_hello
          begin
            rows[name] = compile_one(name, rb)
            r = rows[name]
            progress(format("%-22s %6dms  %6d lines  %5d MiB  %8d bytes",
                            name, r[:frontend_ms], r[:lines], r[:peak_rss] / (1024 * 1024),
                            r[:bin_bytes]))
          rescue Error => e
            progress(format("%-22s FAIL  %s", name, e.message))
            return 1
          end
        end
        return puts("no programs matched") || 1 if rows.empty?

        if opts[:update]
          write_compile_baseline(rows)
          puts "baseline updated: #{path(COMPILE_BASELINE)}"
        end
        0
      end

      def write_hello
        p = File.join(Dir.tmpdir, "zeo-bench-hello.rb")
        File.write(p, %(puts "hello"\n))
        p
      end

      def compile_one(name, rb)
        emitted = File.join(Dir.tmpdir, "zeo-bench-#{name}.clif")
        best_ms = nil
        lines = 0
        peak = 0
        opts[:runs].times do
          t0 = Process.clock_gettime(Process::CLOCK_MONOTONIC)
          res = Exec.run([@zeo, rb, "--emit-clif=#{emitted}", "-W0"], env: { "ZEO_TIMINGS" => "1" })
          ms = ((Process.clock_gettime(Process::CLOCK_MONOTONIC) - t0) * 1000).round
          raise Error, "zeo --emit-clif failed: #{res.stderr.lines.first&.strip}" unless res.success?

          lines = timing_field(res.stderr, "lines=") || 0
          peak = [peak, timing_field(res.stderr, "peak_rss=") || 0].max
          best_ms = best_ms.nil? ? ms : [best_ms, ms].min
        end
        FileUtils.rm_f(emitted)

        # One `-o` for the binary size. Nothing caches a built binary, so
        # this measures the real cost.
        bin = File.join(Dir.tmpdir, "zeo-bench-bin-#{name}")
        res = Exec.run([@zeo, rb, "-o", bin, "-W0"])
        raise Error, "zeo -o failed: #{res.stderr.lines.reject { |l| l.start_with?("zeo-timings:") }.first&.strip}" unless res.success?

        bytes = File.size(bin)
        FileUtils.rm_f(bin)
        { frontend_ms: best_ms, lines: lines, peak_rss: peak, bin_bytes: bytes }
      end

      # One `key=value` field off the last `zeo-timings:` line carrying it.
      def timing_field(stderr, key)
        stderr.lines.reverse_each do |l|
          next unless l.start_with?("zeo-timings:")

          tok = l.split.find { |t| t.start_with?(key) }
          next unless tok

          return tok.delete_prefix(key).delete_suffix("ms").to_i
        end
        nil
      end

      def write_compile_baseline(rows)
        out = +"# bench/compile-baseline.tsv -- zeo-dev bench --compile --update-baseline\n"
        out << "# name\tfrontend_ms\tlines\tpeak_rss\tbin_bytes\n"
        rows.keys.sort.each do |n|
          r = rows[n]
          out << Tsv.row(n, r[:frontend_ms], r[:lines], r[:peak_rss], r[:bin_bytes]) << "\n"
        end
        Tsv.write(path(COMPILE_BASELINE), out)
      end

      # ---- the data files ----------------------------------------------

      # `name\tseconds` rows; `#` lines are comments. A missing file is an
      # empty baseline, and every bench then reports "(no baseline)".
      def read_pairs(p)
        return {} unless File.exist?(p)

        File.readlines(p, chomp: true).each_with_object({}) do |l, h|
          next if l.start_with?("#") || l.strip.empty?

          name, secs = l.split("\t", 2)
          h[name] = secs.to_f if secs
        end
      end

      # Rewrite the whole file, sorted by name for a stable reviewable diff.
      # Called per-benchmark; the file is small, so a full rewrite is simpler
      # and safer than append bookkeeping.
      def write_pairs(p, header, rows)
        out = +"#{header}\n"
        rows.keys.sort.each { |n| out << format("%s\t%.3f\n", n, rows[n]) }
        File.write(p, out)
      end

      # The rolling per-benchmark window: `name\tunix_ts\tsecs` rows, oldest
      # first within a name, capped at 5 per name on write.
      def read_history
        p = path(HISTORY)
        return {} unless File.exist?(p)

        File.readlines(p, chomp: true).each_with_object({}) do |l, h|
          next if l.start_with?("#") || l.strip.empty?

          name, ts, secs = l.split("\t", 3)
          next unless name && ts && secs

          (h[name] ||= []) << [ts.to_i, secs.to_f]
        end
      end

      def write_history(history)
        out = +"# bench/history.tsv -- last 5 timings per benchmark (name, unix_ts, secs)\n"
        history.keys.sort.each do |name|
          history[name].each { |ts, secs| out << format("%s\t%d\t%.3f\n", name, ts, secs) }
        end
        File.write(path(HISTORY), out)
      end

      def median(values)
        v = values.sort
        return Float::NAN if v.empty?

        v.size.odd? ? v[v.size / 2] : (v[v.size / 2 - 1] + v[v.size / 2]) / 2.0
      end
    end
  end
end
