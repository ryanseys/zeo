//! `zeo gem`, `zeo bundle`, `zeo install`: the vendored libraries' own
//! binstubs, as programs zeo compiles and runs.
//!
//! In the library rather than beside the argument parser because the parity
//! probe (`tests/e2e/bundler_parity.rs`) runs the SAME driver text under
//! CRuby. Two copies of it would let the probe pass while the two engines ran
//! different programs, which is the one thing that probe exists to rule out.

/// `zeo gem ...` -- the vendored RubyGems, compiled and run.
///
/// This is `gem`'s own binstub, not a reimplementation: `Gem::GemRunner`
/// parses the arguments, resolves against rubygems.org, downloads, verifies
/// the checksums and installs. What zeo supplies is the ability to run it
/// without a ruby.
pub const GEM_DRIVER: &str = "require \"rubygems\"\nrequire \"rubygems/gem_runner\"\n\
                              Gem::GemRunner.new.run(ARGV)\n";

/// `zeo bundle ...` -- the vendored Bundler, likewise. Its binstub's own
/// shape, `--help` reformatting included, because `bundle install --help` is
/// how people read it.
pub const BUNDLE_DRIVER: &str = "require \"bundler\"\nrequire \"bundler/friendly_errors\"\n\
     Bundler.with_friendly_errors do\n\
     \x20 require \"bundler/cli\"\n\
     \x20 help = ARGV.any? { |a| a == \"--help\" || a == \"-h\" }\n\
     \x20 args = help ? Bundler::CLI.reformatted_help_args(ARGV) : ARGV\n\
     \x20 Bundler::CLI.start(args, debug: true)\n\
     end\n";

/// The driver for a `zeo <name> ...` subcommand, and whether the name itself
/// stays in the driver's `ARGV`.
///
/// The rule is deliberately not "unless a file by that name exists": that
/// would make the same command line mean different things in different
/// directories. A script really called `gem` still runs as `zeo ./gem`.
///
/// `install` is `bundle install` under its own name -- the verb people reach
/// for, and the one every other language's tool spells the same way. It keeps
/// its name so Bundler still sees the subcommand it dispatches on; the other
/// two name the library, which its `ARGV` must not contain.
pub fn driver(name: Option<&str>) -> Option<(&'static str, bool)> {
    match name? {
        "gem" => Some((GEM_DRIVER, false)),
        "bundle" | "bundler" => Some((BUNDLE_DRIVER, false)),
        "install" => Some((BUNDLE_DRIVER, true)),
        _ => None,
    }
}
