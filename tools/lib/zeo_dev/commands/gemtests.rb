# frozen_string_literal: true

module ZeoDev
  module Commands
    # Fetch the WHOLE source trees of the gems whose test suites the
    # `gemtests` golden suite runs end-to-end
    # (`crates/zeo/tests/slow_goldens.rs` over `tests/gemtests/<gem>/*.rb`).
    #
    # A `.gem` archive does not ship `test/` -- the gemspec's `files=`
    # excludes it -- so the trees come from the upstream git repos, pinned in
    # `upstream.rb` as `gemtest` entries. The vendor copies the FULL checkout
    # (`lib/`, `test/`, fixtures and all) into `vendor/gemtests/<name>/`,
    # which is gitignored: fetch-on-demand, and the suite skips gracefully
    # when it is absent.
    #
    # A stub gemspec is written so `vendor/gemtests/` doubles as a `--gems`
    # package dir -- the driver's `require "rack"` resolves against the
    # vendored `lib/` on the zeo side exactly as `-I .../lib` does for the
    # CRuby oracle.
    class Gemtests < Gem
      SUBCOMMANDS = %w[sync outdated lock].freeze

      def self.summary = "fetch the gem source trees the gemtests suite runs"

      def self.banner = "usage: zeo-dev gemtests <sync|outdated|lock> [<name>]"

      private

      def group = :gemtests

      def gems_dir(name) = File.join(ROOT, "vendor", "gemtests", name)

      def materialize(checkout, entry, dest: nil)
        Vendor.vendor_tree(checkout, dest || gems_dir(entry.name), entry)
      end

      # A gemtest tree is gitignored, so there is no committed copy to diff
      # against -- `--check` on this group would always report drift.
      def cmd_sync
        raise Error, "gemtests sync has no --check: the trees are gitignored" if opts[:check]

        entries = manifest.group(group)
        only = args.shift
        entries = entries.select { |e| e.name == only } if only
        raise Error, "#{only} is not a gemtest entry in #{Manifest::SOURCE}" if entries.empty?

        entries.each do |entry|
          unless entry.rev
            raise Error, "#{Manifest::SOURCE}: gemtest #{entry.name} has no rev pin " \
                         "(add one, as the gem entries carry)"
          end
        end
        sync_entries(entries, write: true)
      end
    end
  end
end
