# frozen_string_literal: true

module ZeoDev
  module Commands
    # Re-record goldens from the ruby oracle, for the tests you name and no
    # others.
    #
    # Blessing used to be `ZEO_BLESS=1 cargo test`, and the missing word there
    # is WHICH. An unfiltered run rewrote, created and DELETED goldens across
    # the whole suite in one go. Nothing about the spelling suggested that:
    # `ZEO_BLESS=1` reads like a mode, not like "and apply it to all 5000 of
    # them". The filter below is that guard, and it is still required.
    #
    # Recording runs HERE rather than inside the test binary. Driving it
    # through `cargo nextest` cost ~53 seconds to write one file -- the whole
    # of it cargo, since the dev loop builds `release` and the test profile is
    # `debug`, so every bless after a build paid for a fresh debug compile of
    # zeo and its test binaries. The test itself took 0.18s.
    #
    # Nothing here needs the compiler: recording a golden is "run the oracle,
    # write what it said", and the check that the recording was RIGHT is the
    # suite, which is where it belongs.
    #
    # `norm` is still ported, for a reason that is not correctness: the
    # harness normalizes both sides at compare time, so an unscrubbed
    # recording passes -- but object addresses and thread ids are
    # process-random, so it would rewrite those goldens with fresh noise
    # every run. A bless has to be a no-op when nothing changed.
    #
    # There is deliberately no `--allow-delete`. Removing `<rb>.err.expected`
    # is part of a CORRECT bless -- an absent file is how a golden says
    # "stderr must be empty" -- so gating it would break the ordinary
    # single-test case this exists to make easy. The filter bounds the blast
    # radius; the summary is what makes a deletion impossible to miss.
    class Bless < Cli
      # Everything a bless can write.
      WATCHED = %w[tests].freeze

      # The golden suites, as `datatest_stable::harness!` declares them: the
      # name prefix a filter matches against, the root, and whether cases may
      # sit in a subdirectory. Kept in step with `crates/zeo/tests/*.rs`.
      SUITES = [
        { prefix: "example", root: "tests", glob: "*.rb" },
        { prefix: "divergence", root: "tests/divergences", glob: "*.rb", records: :zeo },
        { prefix: "macos_only", root: "tests/macos", glob: "*.rb" },
        { prefix: "jit_only", root: "tests/jit", glob: "*.rb" },
        { prefix: "gap", root: "tests/gaps", glob: "*.rb" },
        { prefix: "spinel", root: "tests/spinel", glob: "*.rb" },
        { prefix: "milestone", root: "tests/milestones", glob: "{,pending/}*.rb",
          zeo_reads_store: true },
        { prefix: "gemtest", root: "tests/gemtests", glob: "*/*.rb" },
      ].freeze

      # Where `make install-deps` resolves Gemfile.lock.
      BUNDLE = File.join("vendor", "bundle")

      def self.summary = "re-record golden .expected files from the ruby oracle"

      def self.banner = <<~TEXT
        usage: zeo-dev bless <filter>

        <filter> is a substring of the test name -- `<suite>::<path>`, the
        same spelling nextest reports -- and it is required: blessing
        everything at once is what this command exists to prevent. Examples:

          tools/zeo-dev bless forward_args
          tools/zeo-dev bless spinel::yield_
          tools/zeo-dev bless gap::

        A `tests/divergences/` golden records ZEO's output instead of the
        oracle's, and needs a built `target/release/zeo` (or `ZEO_BIN`).
      TEXT

      # Filters that would defeat the point.
      def self.too_broad(filter)
        case filter.strip
        when "" then "an empty filter selects every test"
        when "*", "all" then "this is a substring, not a glob -- it selects every test"
        end
      end

      def run
        filter = args.reject { |a| a.start_with?("-") }.first
        raise Error, self.class.banner if filter.nil?

        if (why = self.class.too_broad(filter))
          raise Error, "bless: refusing #{filter.inspect} -- #{why}"
        end

        cases = select_cases(filter)
        if cases.empty?
          warn "bless: no golden matches #{filter.inspect}"
          return 0
        end

        before = changed_goldens
        warn "bless: re-recording #{cases.size} golden(s) matching #{filter.inspect}"
        # Each case is one oracle process writing its own two files, so the
        # work is subprocess-bound and shares nothing. Recording the whole
        # examples suite serially is ~52s of ruby startup; at core width it
        # is a few seconds.
        Jobs.new.each_parallel(cases.to_a) { |rb, suite| record(rb, suite) }
        report(before, changed_goldens, filter)
      end

      private

      # Every golden whose `<suite>::<relative path>` name contains `filter`.
      def select_cases(filter)
        seen = {}
        SUITES.each do |s|
          root = File.join(ROOT, s[:root])
          Dir.glob(s[:glob], base: root).sort.each do |rel|
            next unless "#{s[:prefix]}::#{rel}".include?(filter)

            seen[File.join(root, rel)] ||= s
          end
        end
        seen
      end

      # Goldens run with the tests directory as cwd (`golden::tests_run_cwd`),
      # which is what a relative path inside one resolves against.
      def run_cwd = File.join(ROOT, "tests")

      # The program is given the ABSOLUTE path, exactly as the harness gives
      # it, and `norm` rewrites that back to the run-cwd-relative form.
      #
      # Handing it the relative path instead looks equivalent and is not:
      # what a program derives from `__FILE__`/`$0` changes with it. rspec
      # prints `./milestones/foo.rb` from an absolute argument and
      # `milestones/foo.rb` from a relative one, so the shortcut silently
      # rewrote that golden.
      def rel_path(rb) = rb.delete_prefix("#{run_cwd}/")

      def record(rb, suite)
        source = File.read(rb)
        argv = sidecar(rb, ".args")&.split || []
        stdin = sidecar_bytes(rb, ".stdin")
        out, err = if suite[:records] == :zeo
                     run_zeo(rb, source, argv, stdin, suite)
                   else
                     run_oracle(rb, source, argv, stdin, suite)
                   end
        File.binwrite("#{rb}.expected", out)
        err_path = "#{rb}.err.expected"
        # An absent `.err.expected` is a real assertion: "stderr must be
        # empty". So an empty capture DELETES rather than writing nothing.
        if err.empty?
          File.delete(err_path) if File.exist?(err_path)
        else
          File.binwrite(err_path, err)
        end
      end

      def sidecar(rb, suffix)
        p = "#{rb}#{suffix}"
        File.read(p) if File.exist?(p)
      end

      def sidecar_bytes(rb, suffix)
        p = "#{rb}#{suffix}"
        File.binread(p) if File.exist?(p)
      end

      # The oracle invocation the harness uses, flag for flag: no
      # error_highlight or did_you_mean (zeo implements neither), the empty
      # gem store and `-I` roots that make the oracle hermetic
      # (`Ruby.oracle_env`), and the experimental-namespace flags a
      # `Ruby::Box` example needs -- naming the class is not enough, because
      # CRuby's disabled-mode surface is a smaller one.
      def run_oracle(rb, source, argv, stdin, suite)
        cmd = Ruby.oracle_argv
        cmd << "-W:no-experimental" if source.include?("Ruby::Box")
        env = Ruby.oracle_env
        env["RUBY_BOX"] = "1" if source.include?("Ruby::Box.new")
        capture(env, [*cmd, rb, *argv], stdin, rb)
      end

      # `vendor/bundle`'s rspec trees, for the one suite whose ZEO side needs
      # those gems too. The oracle reaches them through `Ruby.oracle_env`.
      # Scoped to rspec rather than the whole bundle, which would put
      # upstream copies of gems zeo vendors ahead of its own.
      def gem_store_libs
        Dir.glob(File.join(ROOT, BUNDLE, "ruby", "*", "gems", "{rspec,diff-lcs}*", "lib")).sort
      end

      # A `tests/divergences/` golden records zeo's own answer on purpose.
      # Recording the oracle's would replace the golden with the very output
      # the program exists to differ from.
      #
      # The run-time dials the harness gives zeo have to be given here too,
      # or the recording is of a program that refused to start: without
      # `RUBY_BOX` the box goldens record "Ruby Box is disabled" as their
      # stderr, which then asserts that forever.
      def run_zeo(rb, source, argv, stdin, suite)
        bin = Ruby.zeo
        unless File.executable?(bin)
          raise Error, "bless: #{rb} is a decided divergence and records zeo, but " \
                       "#{bin} is not built -- `cargo build --release -p zeo` first"
        end

        env = {}
        env["RUBY_BOX"] = "1" if source.include?("Ruby::Box.new")
        env["ZEO_GC"] = "1" if File.exist?("#{rb}.gc")
        env["ZEO_RT_LEAKCHECK"] = "1" if File.exist?("#{rb}.leakcheck")
        argv = [*gem_store_libs.flat_map { |d| ["-I", d] }, *argv] if suite[:zeo_reads_store]
        capture(env, [bin, rb, *argv], stdin, rb)
      end

      # `Exec.run` always captures stderr; stdout needs asking for.
      def capture(env, argv, stdin, rb)
        res = Exec.run(argv, env: env, chdir: run_cwd, stdin: stdin, capture_stdout: true)
        [norm(res.stdout.to_s, rb), norm(res.stderr.to_s, rb)]
      end

      # `golden::norm`, ported. The harness applies it to BOTH sides at
      # compare time, so recording without it still PASSES -- but object
      # addresses and thread ids are process-random, so an unscrubbed
      # recording rewrites those goldens with fresh noise on every run. A
      # bless has to be a no-op when nothing changed, which is what makes
      # its summary readable.
      def norm(s, rb)
        s = s.gsub("\r\n", "\n")
        # Both engines embed the absolute source path in `__FILE__` and in
        # backtraces; the committed form is run-cwd-relative so a golden is
        # portable.
        s = s.gsub(rb, rel_path(rb))
        # `0x` + 8..16 hex digits: ruby's own `#<Object:0x...>` (16) and the
        # ASLR'd frame addresses a Rust abort prints (9-12). The run must END
        # there -- a LONGER run is a value, not an address, and taking its
        # first 16 digits turned `0x400000000000000000` into `0xADDR00`.
        # Shorter runs stay too: a program printing `0x1f` keeps its value.
        s = s.gsub(/0x[0-9a-f]{8,16}(?![0-9a-f])/, "0xADDR")
        # `thread 'ruby-main' (156051069) panicked` -- the OS thread id
        # differs per process.
        s.gsub(/' \(\d+\) panicked/, "' (TID) panicked")
      end

      def changed_goldens
        # `-uall`, because git COLLAPSES an untracked directory to one entry:
        # a whole new golden dir read as a single unchanged line before and
        # after, so a bless that wrote its first goldens reported nothing.
        out = Exec.run(%w[git status --porcelain -uall --] + WATCHED, chdir: ROOT,
                                                                      capture_stdout: true)
        return [] unless out.success?

        out.stdout.lines(chomp: true).filter_map do |l|
          next if l.length < 4

          [l[0, 3].strip, l[3..]]
        end
      end

      def report(before, after, filter)
        changed = after - before
        if changed.empty?
          warn "bless: no golden changed -- #{filter.inspect} already records the oracle"
          return 0
        end
        warn "bless: #{changed.size} golden(s) changed:"
        changed.each { |code, path| warn format("  %2s  %s", code, path) }
        deleted = changed.count { |code, _| code.include?("D") }
        if deleted.positive?
          # An absent `.err.expected` is a real assertion ("stderr must be
          # empty"), so a deletion changes the contract as much as a rewrite
          # -- and it is the one change `git diff` shows nothing for.
          warn "\nbless: #{deleted} golden(s) were DELETED. That asserts their stderr is now " \
               "empty;\nif that is not what you meant, `git checkout` them."
        end
        0
      end
    end
  end
end
