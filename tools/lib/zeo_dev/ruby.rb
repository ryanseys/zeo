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
