# An `autoload` whose target is compiled as a UNIT never runs it: the
# constant read raises where ruby loads the file.
#
# This is RubyGems blocker 1, reduced from a 55-second compile to this.
# `require "rubygems"` dies with `uninitialized constant Gem::Requirement`
# at `specification.rb:136`, and pre-requiring `rubygems/requirement` by
# hand walks straight past it.
#
# WHAT IS TRUE, measured 2026-08-26 -- and the recorded diagnosis was wrong
# on two counts, so read this one before following it.
#
#   * The demand IS recorded. `single_unit_demand` gets `thing.rb`, the
#     dedup does NOT drop it, and a unit is compiled for it. The recorded
#     claim that a positional splice claims the dedup slot and drops the
#     demand does not reproduce.
#   * The autoload TOUCH is what is missing. `emit_autoload_touch` fires
#     only for a constant the compiler RESOLVED to a class id, and a read
#     of `Demo::Thing` resolves to nothing here -- so no touch is emitted,
#     no unit runs, and the runtime lookup raises.
#
# The FEATURE SPELLING matters and is why this file uses ruby's own. With a
# bare `autoload :Thing, "thing"` the target is eagerly spliced by some
# other require and the constant simply exists, which is why a first
# reproducer passed and proved nothing. RubyGems writes
# `File.expand_path("rubygems/requirement", __dir__)`, and so does this.
#
# Ruby's answer: reading the constant loads the file, whichever spelling
# named it, and whether or not anything else ever requires it.

$LOAD_PATH.unshift(File.join(__dir__, "an_autoload_target_compiled_as_a_unit"))
require "demo"
require "spec"
p Demo::Thing::VALUE
p Demo::Spec.value
p Demo.autoload?(:Thing).nil?
