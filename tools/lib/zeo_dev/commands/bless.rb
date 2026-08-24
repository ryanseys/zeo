# frozen_string_literal: true

module ZeoDev
  module Commands
    # Re-record goldens from the ruby oracle, for the tests you name and no
    # others.
    #
    # Blessing used to be `ZEO_BLESS=1 cargo test`, and the missing word there
    # is WHICH. An unfiltered run rewrote, created and DELETED goldens across
    # the whole suite in one go, and encoded one machine's gem store into the
    # ones it touched. Nothing about the spelling suggested that: `ZEO_BLESS=1`
    # reads like a mode, not like "and apply it to all 5000 of them".
    #
    # The guard cannot live in the test binary. `zeo-tests` sets
    # `harness = false` and its goldens are datatest-stable cases, so under
    # nextest each case runs in its own process -- a case cannot see whether
    # the user narrowed the run, only that it is itself running. So the filter
    # is owned by whatever spells the invocation, which is this command.
    #
    # There is deliberately no `--allow-delete`. Removing `<rb>.err.expected`
    # is part of a CORRECT bless -- an absent file is how a golden says
    # "stderr must be empty" -- so gating it would break the ordinary
    # single-test case this exists to make easy. The filter bounds the blast
    # radius; the summary is what makes a deletion impossible to miss.
    class Bless < Cli
      # The handshake `zeo-tests` looks for, named for its only legitimate
      # source -- so a bare `ZEO_BLESS=1 cargo test` does nothing and the
      # spelling says where to go instead.
      BLESS_VAR = "ZEO_BLESS_FROM_TOOL"

      # Goldens and generated conformance artifacts: everything a bless can
      # write.
      WATCHED = %w[tests conformance].freeze

      def self.summary = "re-record golden .expected files from the ruby oracle"

      def self.banner = <<~TEXT
        usage: zeo-dev bless <filter> [-- <nextest args>]

        <filter> is a nextest substring match on the test name, and it is
        required: blessing everything at once is what this command exists to
        prevent. Examples:

          tools/zeo-dev bless forward_args
          tools/zeo-dev bless spinel::yield_
      TEXT

      # Filters that would defeat the point. nextest's `test()` matcher is a
      # substring, so the empty string selects everything.
      def self.too_broad(filter)
        case filter.strip
        when "" then "an empty filter selects every test"
        when "*", "all" then "this is a substring, not a glob -- it selects every test"
        end
      end

      def run
        filter, passthrough = split_args
        raise Error, self.class.banner if filter.nil?

        if (why = self.class.too_broad(filter))
          raise Error, "bless: refusing #{filter.inspect} -- #{why}"
        end

        before = changed_goldens
        warn "bless: re-recording goldens matching #{filter.inspect} from the ruby oracle"
        # `-p zeo-tests` is load-bearing for SPEED, not scope: every reader of
        # BLESS_VAR lives in that package, and an unscoped nextest resolves
        # features across the whole workspace, which invalidates the build
        # every time.
        #
        # A blessing run is EXPECTED to report failures: a case that rewrites
        # its golden and then asserts against the old one is not the contract.
        # What matters is what changed on disk.
        system({ BLESS_VAR => "1" },
               "cargo", "nextest", "run", "-p", "zeo-tests",
               "-E", "test(#{filter})", *passthrough, chdir: ROOT)

        report(before, changed_goldens, filter)
      end

      private

      # Everything before a bare `--` is the filter; a flag anywhere and
      # everything after `--` goes to nextest.
      def split_args
        filter = nil
        passthrough = []
        rest = args.dup
        until rest.empty?
          a = rest.shift
          if a == "--"
            passthrough.concat(rest)
            break
          elsif a.start_with?("-")
            passthrough << a
          elsif filter.nil?
            filter = a
          else
            raise Error, "bless: unexpected second filter #{a.inspect}"
          end
        end
        [filter, passthrough]
      end

      def changed_goldens
        out = Exec.run(%w[git status --porcelain --] + WATCHED, chdir: ROOT, capture_stdout: true)
        return [] unless out.success?

        out.stdout.lines(chomp: true).filter_map do |l|
          next if l.length < 4

          [l[0, 3].strip, l[3..]]
        end
      end

      def report(before, after, filter)
        changed = after - before
        if changed.empty?
          warn "bless: no golden changed -- was #{filter.inspect} the name you meant?"
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
