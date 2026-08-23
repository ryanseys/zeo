# frozen_string_literal: true

require "set"

module ZeoDev
  module Commands
    # Every environment variable the docs name must have a reader in the tree.
    #
    # A documented variable with no reader is a silent no-op: the reader was
    # renamed or deleted and the instruction outlived it. Three of those were
    # found at once -- `ZEO_BLESS=1` had been replaced by
    # `ZEO_BLESS_FROM_XTASK`, and two ledgers plus a doc still told the reader
    # to use the old spelling, which does nothing at all.
    class CheckEnvVars < Cli
      # Spellings the docs mention only to say they are retired. A variable
      # named to say it does NOT work is not a broken instruction.
      HISTORICAL = %w[ZEO_BLESS].freeze

      def self.summary = "every documented ZEO_* variable has a reader"
      def self.banner = "usage: zeo-dev check-env-vars"

      def run
        expect_no_args!
        named = documented_names - HISTORICAL
        # A reader spells the name as a string literal: `env::var("X")`,
        # `var_os("X")`, `ENV["X"]`.
        missing = named.reject { |v| readers.include?(v) }
        if missing.empty?
          puts "every documented ZEO_* variable has a reader (#{named.size} checked)"
          return 0
        end
        warn "::error::documented environment variables with no reader:"
        missing.each { |v| warn "  #{v}" }
        warn "Either restore the reader, correct the spelling, or add it to HISTORICAL."
        1
      end

      private

      def docs
        %w[README.md CONTRIBUTING.md].map { |f| File.join(ROOT, f) }.select { |f| File.file?(f) } +
          %w[docs tests].flat_map do |dir|
            Dir.glob("#{dir}/**/*.{md,tsv}", base: ROOT).map { |r| File.join(ROOT, r) }
          end
      end

      def documented_names
        docs.flat_map { |f| File.read(f).scan(/\bZEO_[A-Z0-9_]+\b/) }.uniq.sort
      rescue ArgumentError
        # A .tsv can hold bytes that are not UTF-8; scan them as binary.
        docs.flat_map { |f| File.binread(f).scan(/\bZEO_[A-Z0-9_]+\b/) }.uniq.sort
      end

      # Every string literal in the source that could be a reader, gathered
      # once rather than grepped per name.
      def readers
        @readers ||= %w[crates tools].flat_map do |dir|
          Dir.glob("#{dir}/**/*.{rs,rb}", base: ROOT).flat_map do |rel|
            File.binread(File.join(ROOT, rel)).scan(/"(ZEO_[A-Z0-9_]+)"/).flatten
          end
        end.to_set
      end
    end
  end
end
