# frozen_string_literal: true

# zeo, as a RubyGem.
#
# Two gems come out of this one file, and which one depends on what is beside
# it when `gem build` runs:
#
#   SOURCE gem      built here, in the repo. Carries the launcher and the
#                   docs and no binary, because there is nothing to build
#                   from a Ruby packaging step -- zeo is Rust. Installing it
#                   gives a `zeo` that says which platform gem to fetch.
#   PLATFORM gem    built by `cargo xtask gem` from a staging directory that
#                   already holds `libexec/zeo` and `share/zeo/`. Those are
#                   picked up by the globs below and the platform comes from
#                   `ZEO_GEM_PLATFORM`.
#
# The staged layout is `cargo xtask dist`'s tree with `bin/` renamed
# `libexec/`:
#
#   exe/zeo                             the Ruby launcher (RubyGems binstubs it)
#   libexec/zeo                         the native binary
#   share/zeo/lib/ruby/**               every bundled library
#   share/zeo/lib/<triple>/libzeo.a     what `zeo -o` links a program against
#   share/zeo/dist-manifest.json
#
# `libexec/` because RubyGems binstubs an executable by `load`ing it as Ruby,
# and zeo is a native binary. Both sit two levels above `share/zeo`, so a gem
# install and a release tarball put the same bytes in the same shape and
# `zeo::home`'s executable-relative probe finds the payload in both.
#
# NO `add_dependency` LINES, deliberately. zeo bundles a stdlib -- uri, csv,
# json and ~70 more -- but bundling is not depending: `gem install zeo` must
# not force uri 1.1.1 into a user's store or collide with a project's own
# pins, and for the libraries zeo REIMPLEMENTS in Rust a dependency would be
# a false claim, because zeo does not run that code. `add_dependency` is for
# what a gem needs INSTALLED, and zeo needs none of these installed.

root = __dir__

# One version, from whichever file states it here. A staging directory has
# the manifest `cargo xtask dist` wrote; the repo has `Cargo.toml`, which is
# where that manifest's number came from. Neither is invented.
manifest = File.join(root, "share/zeo/dist-manifest.json")
version =
  if File.file?(manifest)
    File.read(manifest)[/"version":\s*"([^"]+)"/, 1]
  else
    File.read(File.join(root, "Cargo.toml"))[/^\s*version\s*=\s*"([^"]+)"/, 1]
  end
raise "no version in #{manifest} or Cargo.toml" unless version

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
    "changelog_uri" => "#{s.homepage}/blob/main/CHANGELOG.md",
    "documentation_uri" => "#{s.homepage}/blob/main/docs/README.md",
    # Nothing here is loadable Ruby, so there is no point indexing it.
    "rubygems_mfa_required" => "true"
  }

  # What zeo COMPILES is a much wider range than what runs this launcher.
  # The launcher is `exec`, so anything with a working `Gem::Platform` does.
  s.required_ruby_version = ">= 3.1"

  s.bindir = "exe"
  s.executables = ["zeo"]
  # zeo ships no Ruby to require, but RubyGems refuses a spec with no require
  # path at all, so this is the conventional one and it stays empty.
  s.require_paths = ["lib"]

  s.files = Dir.chdir(root) do
    docs = %w[README.md LICENSE-MIT LICENSE-APACHE THIRD-PARTY-NOTICES.md]
    # FNM_DOTMATCH keeps `.keep`-style payload files; the editor and
    # Finder droppings it would also sweep up are rejected by name.
    payload = Dir.glob("{libexec,share}/**/*", File::FNM_DOTMATCH)
                 .select { |f| File.file?(f) }
                 .reject { |f| File.basename(f) == ".DS_Store" }
    docs.select { |f| File.file?(f) } + ["exe/zeo"] + payload
  end
end
