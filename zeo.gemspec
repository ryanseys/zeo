# frozen_string_literal: true

# The rubygems half of one payload, two published artifacts.
#
# `cargo xtask gem` stages a directory that already IS this layout and then
# builds from here, so the file list below describes a staged tree rather
# than the repo:
#
#   exe/zeo                             the Ruby launcher (RubyGems binstubs it)
#   libexec/zeo                         the native binary
#   share/zeo/lib/ruby/**               every bundled library
#   share/zeo/lib/<triple>/libzeo.a     what `zeo -o` links a program against
#   share/zeo/dist-manifest.json
#
# That is `cargo xtask dist`'s tree with `bin/` renamed `libexec/`, so a gem
# install and a release tarball put the same bytes in the same shape and
# `zeo::home`'s executable-relative probe finds the payload in both.
#
# NO `add_dependency` LINES, deliberately. zeo bundles a stdlib -- uri, csv,
# json and ~70 more -- but bundling is not depending: `gem install zeo` must
# not force uri 1.1.1 into a user's store or collide with a project's own
# pins, and for the libraries zeo REIMPLEMENTS in Rust a dependency would be
# a false claim, because zeo does not run that code. `add_dependency` is for
# what a gem needs INSTALLED, and zeo needs none of these installed.
#
# Which gems a compiled program uses is stated in the README and in
# `zeo --help`: the embedded payload by default, and the project's own
# resolved gems when `--bundle-gemfile` / `--gem-path` name them.

root = __dir__

# Single-sourced from the staged payload, which `dist` writes from
# `Cargo.toml`. Built anywhere else -- the repo root, say -- there is no
# manifest and this raises rather than inventing a version.
manifest = File.join(root, "share/zeo/dist-manifest.json")
unless File.file?(manifest)
  raise "#{manifest} is missing: build this gem from a `cargo xtask gem` " \
        "staging directory, not from the repo"
end
version = File.read(manifest)[/"version":\s*"([^"]+)"/, 1]
raise "#{manifest} states no version" unless version

Gem::Specification.new do |s|
  s.name = "zeo"
  s.version = version
  # `cargo xtask gem` sets this per target. Left unset, the spec is the
  # source gem: no `libexec/zeo`, and `exe/zeo` refuses with the platform
  # named.
  s.platform = ENV["ZEO_GEM_PLATFORM"] || Gem::Platform::RUBY

  s.summary = "An ahead-of-time Ruby compiler"
  s.description = <<~TEXT
    zeo compiles a Ruby program to a self-contained native binary through
    Cranelift. It ships its own stdlib, rubygems and bundler, so a compiled
    program needs no ruby on the machine that runs it.
  TEXT
  s.homepage = "https://github.com/ryanseys/zeo"
  s.licenses = ["MIT", "Apache-2.0"]
  s.authors = ["Ryan Seys"]

  s.metadata = {
    "homepage_uri" => s.homepage,
    "source_code_uri" => s.homepage,
    "bug_tracker_uri" => "#{s.homepage}/issues",
    # Nothing here is loadable Ruby, so there is no point indexing it.
    "rubygems_mfa_required" => "true"
  }

  # What zeo COMPILES is a much wider range than what runs this launcher.
  # The launcher is `exec`, so anything with a working `Gem::Platform` does.
  s.required_ruby_version = ">= 3.1"

  s.bindir = "exe"
  s.executables = ["zeo"]
  s.require_paths = ["lib"]

  s.files = Dir.chdir(root) do
    %w[README.md LICENSE-MIT LICENSE-APACHE THIRD-PARTY-NOTICES.md exe/zeo] +
      Dir.glob("libexec/**/*", File::FNM_DOTMATCH).select { |f| File.file?(f) } +
      Dir.glob("share/**/*", File::FNM_DOTMATCH).select { |f| File.file?(f) }
  end
end
