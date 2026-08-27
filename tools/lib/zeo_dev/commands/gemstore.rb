# frozen_string_literal: true

require "fileutils"

module ZeoDev
  module Commands
    # The two gem stores the ruby oracle runs against. Both generated, both
    # gitignored, both under `vendor/`.
    #
    # The oracle used to run against whatever was installed on the machine,
    # and that quietly decided what goldens recorded: reline 0.7.0 and webrick
    # 1.9.2 sat in this machine's store, so two goldens claimed ruby 4.0.6
    # shipped them. `gem install` drops newer copies beside the shipped ones
    # and `require` activates the newest, so neither `gem list` nor a
    # require-and-print probe reports what ruby actually ships.
    #
    # So the oracle gets exactly two stores and nothing else:
    #
    #   vendor/oracle-gems   what RUBY ships -- its default and bundled gems,
    #                        at its own versions, mirrored by symlink from the
    #                        installation.
    #   vendor/gemstore      what ruby does NOT ship but zeo is compared
    #                        against: `ffi`, which zeo implements itself, and
    #                        `rspec` for the milestone that runs a real suite.
    #
    # Both are `GEM_PATH` entries, so ruby resolves them the ordinary way and
    # nothing has to keep a list of gem names anywhere.
    class Gemstore < Cli
      INSTALLED = File.join("vendor", "gemstore")
      ORACLE = File.join("vendor", "oracle-gems")

      # Pinned, so the store is the same on every machine and in CI. `gem
      # install` resolves the dependencies (rspec pulls its four siblings and
      # diff-lcs), so only the roots are named.
      PINS = { "rspec" => "3.13.2", "ffi" => "1.17.4" }.freeze

      def self.summary = "build the gem stores the ruby oracle runs against"

      def self.banner = <<~TEXT
        usage: zeo-dev gemstore [--check|--clean]

        Installs #{PINS.map { |n, v| "#{n} #{v}" }.join(", ")} into #{INSTALLED},
        and mirrors ruby's OWN shipped gems into #{ORACLE}. Needs the network
        the first time and nothing after it.
      TEXT

      def defaults = { check: false, clean: false }

      def options(o)
        o.on("--check", "report whether the stores are complete; nonzero if not") do
          opts[:check] = true
        end
        o.on("--clean", "delete them first") { opts[:clean] = true }
      end

      def run
        return report if opts[:check]

        [INSTALLED, ORACLE].each { |d| FileUtils.rm_rf(File.join(ROOT, d)) } if opts[:clean]
        build_installed
        build_oracle
        report
      end

      private

      # --- vendor/gemstore: the gems ruby does not ship --------------------

      def build_installed
        return if installed_complete?

        dir = File.join(ROOT, INSTALLED)
        FileUtils.mkdir_p(dir)
        PINS.each { |name, version| install(dir, name, version) }
      end

      # `gem install --install-dir` writes a self-contained store and touches
      # nothing else -- not the machine's gems, not ruby's own.
      def install(dir, name, version)
        argv = [gem_bin, "install", name, "--version", version,
                "--install-dir", dir, "--no-document", "--no-user-install"]
        warn "gemstore: installing #{name} #{version} into #{INSTALLED}"
        res = Exec.run(argv, chdir: ROOT)
        return if res.success?

        raise Error, "gemstore: `gem install #{name}` failed:\n#{res.stderr}"
      end

      # A gem with a C extension installs under a platform suffix
      # (`ffi-1.17.4-arm64-darwin.gemspec`), so the check globs for it.
      def installed_complete?
        PINS.all? do |n, v|
          !Dir.glob(File.join(ROOT, INSTALLED, "specifications", "#{n}-#{v}{,-*}.gemspec")).empty?
        end
      end

      # --- vendor/oracle-gems: what ruby itself ships ----------------------

      # Nothing on disk marks a shipped gem apart from a later install. The
      # one honest signal is the clock: ruby writes every gemspec in one
      # batch, and `specifications/default/` -- a directory ruby owns outright
      # -- dates that batch. A gemspec written in that minute shipped with
      # ruby; anything else did not.
      #
      # Every entry is a SYMLINK, so this costs no disk and follows the
      # installation. Rebuilt from scratch each time, because a stale entry is
      # exactly the failure it exists to prevent.
      def build_oracle
        return warn("gemstore: no `gem env gemdir`; skipping #{ORACLE}") if gemdir.nil?

        dir = File.join(ROOT, ORACLE)
        FileUtils.rm_rf(dir)
        %w[specifications gems extensions].each { |d| FileUtils.mkdir_p(File.join(dir, d)) }
        link(File.join(gemdir, "specifications", "default"),
             File.join(dir, "specifications", "default"))
        shipped_gemspecs.each { |spec| mirror(dir, spec) }
      end

      def mirror(dir, spec)
        stem = File.basename(spec).delete_suffix(".gemspec")
        link(spec, File.join(dir, "specifications", File.basename(spec)))
        link(File.join(gemdir, "gems", stem), File.join(dir, "gems", stem))
        # A gem with a C extension loads only with its built `.bundle`/`.so`,
        # which rubygems looks for under the STORE's own `extensions/`.
        # Without this ruby reads fiddle's Ruby half and then fails on its own
        # `require "fiddle.so"`.
        Dir.glob(File.join(gemdir, "extensions", "*", "*", stem)).each do |ext|
          abi = File.join(dir, "extensions", *ext.split(File::SEPARATOR)[-3, 2])
          FileUtils.mkdir_p(abi)
          link(ext, File.join(abi, stem))
        end
      end

      def link(from, to)
        File.symlink(from, to) if File.exist?(from) && !File.symlink?(to)
      end

      def shipped_gemspecs
        specs = File.join(gemdir, "specifications")
        shipped_at = Dir.glob(File.join(specs, "default", "*.gemspec"))
                        .map { |f| File.mtime(f).strftime("%F %H:%M") }.min
        return [] if shipped_at.nil?

        Dir.glob(File.join(specs, "*.gemspec")).select do |f|
          File.mtime(f).strftime("%F %H:%M") == shipped_at
        end
      end

      # --- reporting -------------------------------------------------------

      def report
        mirrored = Dir.glob(File.join(ROOT, ORACLE, "specifications", "*.gemspec")).size
        unless installed_complete? && mirrored.positive?
          warn "the gem stores are missing or incomplete -- run `tools/zeo-dev gemstore`"
          return 1
        end
        installed = Dir.glob(File.join(ROOT, INSTALLED, "specifications", "*.gemspec")).size
        puts "#{INSTALLED} holds #{installed} gem(s); #{ORACLE} mirrors #{mirrored}"
        0
      end

      # The `mise.toml`-pinned gem, not whatever is on PATH: these stores are
      # read by the conformance oracle, so they must come from that ruby.
      def gem_bin
        bin = `mise which gem 2>/dev/null`.strip
        bin.empty? ? "gem" : bin
      end

      def gemdir = Ruby.gemdir
    end
  end
end
