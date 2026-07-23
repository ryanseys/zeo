# frozen_string_literal: true
#
# Synthesized `rbconfig` shim, built into zeo (spliced by parse/loader.rs when a
# program `require`s "rbconfig"). Real Ruby generates rbconfig.rb at build time
# from its own build configuration; zeo has no such build, so it ships this
# static stand-in describing the target it emulates (ruby 4.0.5). It carries the
# keys rubygems + bundler actually read (version, platform, install layout).
#
# FIRST-PASS LIMITATION: the platform/arch and install paths below are static
# (arm64 macOS, an FHS-style prefix), not derived from the real build target.
# When target-arch fidelity is needed (cross-compiling, gem-install milestones)
# this should become compile-time-generated from zeo's build target. Tracked in
# docs/todo/bundler-northstar.md.
module RbConfig
  ruby_version = "4.0.0"
  prefix = "/usr/local"
  arch = "arm64-darwin25"

  CONFIG = {
    "MAJOR" => "4",
    "MINOR" => "0",
    "TEENY" => "0",
    "PATCHLEVEL" => "5",
    "ruby_version" => ruby_version,
    "RUBY_SO_NAME" => "ruby.4.0",
    "ruby_install_name" => "ruby",
    "RUBY_INSTALL_NAME" => "ruby",
    "RUBY_BASE_NAME" => "ruby",

    # Platform / host triple.
    "arch" => arch,
    "sitearch" => arch,
    "platform" => "arm64-apple-darwin25",
    "host" => "arm64-apple-darwin25",
    "host_alias" => "",
    "host_cpu" => "arm64",
    "host_vendor" => "apple",
    "host_os" => "darwin25",
    "target" => "arm64-apple-darwin25",
    "target_cpu" => "arm64",
    "target_vendor" => "apple",
    "target_os" => "darwin25",

    # Executable / shared-object extensions.
    "EXEEXT" => "",
    "EXECUTABLE_EXTS" => "",
    "DLEXT" => "bundle",
    "SOEXT" => "dylib",
    "LIBEXT" => "a",
    "OBJEXT" => "o",

    # Install layout (prefix-relative; a synthesized default, not a real build).
    "prefix" => prefix,
    "exec_prefix" => prefix,
    "bindir" => "#{prefix}/bin",
    "libdir" => "#{prefix}/lib",
    "rubylibprefix" => "#{prefix}/lib/ruby",
    "rubylibdir" => "#{prefix}/lib/ruby/#{ruby_version}",
    "archdir" => "#{prefix}/lib/ruby/#{ruby_version}/#{arch}",
    "sitedir" => "#{prefix}/lib/ruby/site_ruby",
    "sitelibdir" => "#{prefix}/lib/ruby/site_ruby/#{ruby_version}",
    "sitearchdir" => "#{prefix}/lib/ruby/site_ruby/#{ruby_version}/#{arch}",
    "vendordir" => "#{prefix}/lib/ruby/vendor_ruby",
    "vendorlibdir" => "#{prefix}/lib/ruby/vendor_ruby/#{ruby_version}",
    "vendorarchdir" => "#{prefix}/lib/ruby/vendor_ruby/#{ruby_version}/#{arch}",
    "sysconfdir" => "/etc",
    "mandir" => "#{prefix}/share/man",
    "datadir" => "#{prefix}/share",
    "localstatedir" => "/var",

    # Feature flags rubygems consults.
    "ENABLE_SHARED" => "yes",
    "host_os_version" => "25",
    "CC" => "clang",
    "CXX" => "clang++",
    "LDSHARED" => "clang -dynamic -bundle",
    "DLDFLAGS" => "",
    "configure_args" => "",
  }.freeze

  MAKEFILE_CONFIG = CONFIG.dup
  TOPDIR = CONFIG["prefix"]

  # `RbConfig.ruby` -- the full path to the running interpreter.
  def self.ruby
    File.join(CONFIG["bindir"], CONFIG["ruby_install_name"] + CONFIG["EXEEXT"])
  end

  # `RbConfig.expand` normally substitutes `$(var)` references; every value here
  # is already fully expanded, so this is an identity pass.
  def self.expand(val, _config = CONFIG)
    val
  end
end

RbConfig::CONFIG.freeze
