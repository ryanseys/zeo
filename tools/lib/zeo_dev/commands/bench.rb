# frozen_string_literal: true

require "fileutils"
require "tmpdir"

module ZeoDev
  module Commands
    # The COMPILER-cost instrument: what a compile costs and produces per
    # program (frontend wall time, emitted CLIF lines, peak RSS, binary
    # size) across a fixed hello-world-to-bundler ladder.
    #
    # The RUNTIME performance suite lives in criterion now:
    #
    #   cargo bench -p zeo --bench programs
    #
    # (see bench/README.md for baselines, comparisons, and profiling).
    class Bench < Cli
      # The fixed program set: names paired with repo-relative paths,
      # spanning hello-world to bundler scale. `hello` is synthesized so the
      # smallest possible program is always in the record.
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

      COMPILE_BASELINE = "bench/compile-baseline.tsv"

      def self.summary = "the compiler-cost instrument (runtime suite: cargo bench)"

      def self.banner
        "usage: zeo-dev bench --compile [--filter <substr>] [--runs N] [--update-baseline]"
      end

      def defaults = { filter: nil, runs: 3, update: false, compile: false }

      def options(o)
        o.on("--filter SUBSTR", "only programs whose name contains SUBSTR") { |v| opts[:filter] = v }
        o.on("--runs N", Integer, "compiles per program (default 3)") { |v| opts[:runs] = v }
        o.on("--update-baseline", "bank the measured rows into the baseline") { opts[:update] = true }
        o.on("--compile", "measure the compiler (the only mode)") { opts[:compile] = true }
      end

      def run
        expect_no_args!
        raise Error, "bench: --runs takes a positive integer" unless opts[:runs].positive?
        unless opts[:compile]
          raise Error, "bench: the runtime suite moved to criterion -- " \
                       "run `cargo bench -p zeo --bench programs` " \
                       "(this command keeps only --compile)"
        end

        @zeo = Ruby.build_zeo!
        run_compile
      end

      private

      def path(rel) = File.join(ROOT, rel)

      def selected(name) = opts[:filter].nil? || name.include?(opts[:filter])

      # A progress line the caller sees IMMEDIATELY, even piped or
      # backgrounded: stdout through a pipe is block-buffered.
      def progress(line)
        puts line
        $stdout.flush
      end

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
    end
  end
end
