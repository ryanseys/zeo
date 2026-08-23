# frozen_string_literal: true

module ZeoDev
  module Commands
    # Records what CRuby's whole reachable module tree owns, into
    # `conformance/method-census.tsv`.
    #
    # The dump is checked in so `crates/zeo-tests/tests/method_census.rs` can
    # gate coverage on a machine with no ruby -- that test compiles the SAME
    # walker (`tools/method_census.rb`) through zeo and diffs the two.
    # Regenerating is an explicit act; `--check` re-runs the oracle and fails
    # if the committed copy is stale, for a CI job that does have the pinned
    # ruby.
    class MethodCensus < Cli
      DUMP = "conformance/method-census.tsv"
      WALKER = "tools/method_census.rb"

      def self.summary = "record conformance/method-census.tsv from the oracle"
      def self.banner = "usage: zeo-dev method-census [--check]"

      def defaults = { check: false }

      def options(o)
        o.on("--check", "fail if the committed copy is stale, and write nothing") do
          opts[:check] = true
        end
      end

      def run
        expect_no_args!
        # `--disable-gems` and nothing else: the walker reads the core tree,
        # and any flag added here changes the recorded bytes.
        res = Exec.run([Ruby.oracle, "--disable-gems", File.join(ROOT, WALKER)], chdir: ROOT,
                                                                                capture_stdout: true)
        unless res.success?
          warn "the ruby oracle exited with #{res.code.inspect}"
          warn res.stderr
          return 1
        end
        dump = res.stdout
        target = File.join(ROOT, DUMP)

        return Tsv.check(target, dump, label: DUMP) if opts[:check]

        Tsv.write(target, dump)
        modules = dump.lines.count { |l| l.include?("\ti\t") }
        puts "wrote #{DUMP} (#{modules} modules, #{dump.lines.count} lines)"
        0
      end
    end
  end
end
