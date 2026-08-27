# frozen_string_literal: true

module ZeoDev
  # Locating the two binaries every command needs: the pinned CRuby oracle,
  # and zeo itself.
  module Ruby
    module_function

    # `mise which ruby`, falling back to a bare `ruby` -- the same resolution
    # the golden harness uses, so both come from the `mise.toml`-pinned
    # oracle.
    #
    # This matters more than it looks. A bare `ruby` off PATH is whatever
    # version the shell happens to offer, and a ledger recorded against a
    # different ruby than the goldens came from is a fiction.
    def oracle
      @oracle ||= begin
        out = `mise which ruby 2>/dev/null`.strip
        out.empty? ? "ruby" : out
      rescue StandardError
        "ruby"
      end
    end

    # The oracle's own flags. `error_highlight` and `did_you_mean` rewrite an
    # exception message, and zeo implements neither, so every comparison runs
    # without them.
    ORACLE_FLAGS = ["--disable-error_highlight", "--disable-did_you_mean"].freeze

    def oracle_argv(*args) = [oracle, *ORACLE_FLAGS, *args]

    # `gem env gemdir` for the PINNED ruby, not whatever is on PATH.
    def gemdir
      @gemdir ||= begin
        bin = `mise which gem 2>/dev/null`.strip
        bin = "gem" if bin.empty?
        out = `#{bin} env gemdir 2>/dev/null`.strip
        out.empty? ? nil : out
      end
    end

    # The two generated stores, and nothing else -- see `zeo-dev gemstore`.
    # Whatever anybody has `gem install`ed on this machine is invisible to a
    # golden, which it was not: reline 0.7.0 and webrick 1.9.2 sat in this
    # machine's store and two goldens recorded them as if ruby shipped them.
    def oracle_env
      require "zeo_dev/commands/gemstore"
      mirror = File.join(ROOT, Commands::Gemstore::ORACLE)
      installed = File.join(ROOT, Commands::Gemstore::INSTALLED)
      { "GEM_HOME" => mirror,
        "GEM_PATH" => [mirror, installed].join(File::PATH_SEPARATOR) }
    end

    # The zeo binary a command should drive. `ZEO_BIN` overrides; otherwise
    # the release build, which is what every ledger was recorded against.
    def zeo(profile: "release")
      ENV["ZEO_BIN"] || File.join(ROOT, "target", profile, "zeo")
    end

    # Builds zeo unless it is already current. Answers the path.
    def build_zeo!(profile: "release")
      return ENV["ZEO_BIN"] if ENV["ZEO_BIN"]

      flags = profile == "release" ? ["--release"] : []
      ok = system("cargo", "build", "--quiet", *flags, "-p", "zeo", chdir: ROOT)
      raise Error, "cargo build -p zeo failed" unless ok

      zeo(profile: profile)
    end
  end
end
