# frozen_string_literal: true

require "fileutils"

module ZeoDev
  module Commands
    # Promote a FIXED gap out of `tests/gaps/` into the passing suite.
    #
    # When a gap starts matching ruby, the gaps harness fails it with
    # "GAP FIXED -- promote". This moves the gap's `.rb` and every sidecar
    # into `tests/` -- the zeo-authored golden suite, the `examples` target --
    # and confirms it passes there.
    #
    # `tests/spinel/` is NOT a promotion target: it mirrors a vendored corpus,
    # and a spinel-origin gap belongs in `tests/` like any other.
    class PromoteGap < Cli
      # EVERY suffix the harness recognizes (golden.rs is the reference).
      # This list once knew only five of them, and promoting a gap that
      # carried a `.gccheck`, `.gc` or `.leakcheck` silently left the sidecar
      # behind in tests/gaps/ -- changing the promoted test's behavior and
      # orphaning a file.
      #
      # What a golden IS -- a divergence, macOS-only, JIT-only -- is its
      # DIRECTORY, not a suffix, so a promotion that changes the kind is a
      # move to a different directory and nothing here has to know about it.
      SIDECARS = %w[
        rb rb.expected rb.err.expected rb.args rb.stdin
        rb.gc rb.leakcheck rb.gccheck
      ].freeze

      def self.summary = "move a fixed gap into the passing suite"
      def self.banner = "usage: zeo-dev promote-gap <gap-stem>"

      def run
        stem = args.shift&.delete_suffix(".rb")
        raise Error, self.class.banner if stem.nil?

        gaps = File.join(ROOT, "tests", "gaps")
        dest = File.join(ROOT, "tests")
        raise Error, "no such gap: tests/gaps/#{stem}.rb" unless File.file?(File.join(gaps, "#{stem}.rb"))

        moved = SIDECARS.filter_map do |suf|
          from = File.join(gaps, "#{stem}.#{suf}")
          next unless File.file?(from)

          FileUtils.mv(from, File.join(dest, "#{stem}.#{suf}"))
          "#{stem}.#{suf}"
        end
        puts "promoted tests/gaps/#{stem}.rb -> tests/ (#{moved.size} files: #{moved.join(" ")})"

        puts "verifying it passes in the examples suite ..."
        ok = system("cargo", "nextest", "run", "-p", "zeo", "--test", "examples",
                    "-E", "test(#{stem})", chdir: ROOT)
        ok ? 0 : 1
      end
    end
  end
end
