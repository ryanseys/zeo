# frozen_string_literal: true

module ZeoDev
  module Commands
    # Append per-binary suite timings to `measurements/test-times.tsv`.
    #
    # nextest writes a JUnit report per profile (`.config/nextest.toml`
    # `[profile.*.junit]`); each `<testsuite>` element there is one test
    # binary with its case count and wall-clock total. This folds the most
    # recent report into a durable, gitignored ledger, so "did the suites
    # get slower" has a measured answer instead of a remembered one -- the
    # build side of this repo is measured to two decimals and the test side
    # was not measured at all.
    #
    # A nextest run OVERWRITES its profile's report, so the caller labels
    # what the current report describes (`make test` passes its own name).
    # Parsing is a line regexp, not an XML library: the tools are stdlib-only
    # and the attributes nextest writes sit on one `<testsuite ...>` line.
    class TestTimes < Cli
      TSV = File.join(ROOT, "measurements", "test-times.tsv")
      HEADER = %w[date sha label profile binary tests seconds].join("\t")

      def self.summary = "append the last nextest run's per-binary times to measurements/"
      def self.banner = "usage: zeo-dev test-times <label> [profile]"

      def run
        label = args.shift or raise Error, self.class.banner
        profile = args.shift || "default"
        junit = File.join(ROOT, "target", "nextest", profile, "junit.xml")
        unless File.file?(junit)
          warn "no report at #{junit} (is [profile.#{profile}.junit] configured?)"
          return 1
        end
        date = Time.now.strftime("%Y-%m-%dT%H:%M:%S")
        sha = `git -C #{ROOT} rev-parse --short HEAD 2>/dev/null`.strip
        rows = suites(junit).map do |name, tests, seconds|
          [date, sha, label, profile, name, tests, seconds].join("\t")
        end
        if rows.empty?
          warn "no <testsuite> rows in #{junit}"
          return 1
        end
        Dir.mkdir(File.dirname(TSV)) unless Dir.exist?(File.dirname(TSV))
        File.write(TSV, "#{HEADER}\n") unless File.file?(TSV)
        File.open(TSV, "a") { |f| rows.each { |r| f.puts(r) } }
        puts "recorded #{rows.size} suite rows -> #{TSV}"
        0
      end

      private

      # `[["zeo::gaps", "76", "2.49"], ...]`. A `<testsuite>` row carries no
      # time of its own, so each suite's total is the sum of its
      # `<testcase time=...>` children -- CPU-seconds of test work, which
      # under nextest's parallelism is more stable than wall clock anyway.
      def suites(junit)
        rows = []
        current = nil
        File.foreach(junit) do |line|
          if line =~ /<testsuite\s/
            rows << current if current
            current = [line[/name="([^"]+)"/, 1], line[/tests="(\d+)"/, 1], 0.0]
          elsif current && line =~ /<testcase\s.*time="([0-9.]+)"/
            current[2] += Regexp.last_match(1).to_f
          end
        end
        rows << current if current
        rows.select { |name, tests, _| name && tests }
            .map { |name, tests, secs| [name, tests, secs.round(2)] }
      end
    end
  end
end
