# An `autoload` declared inside a file that is itself compiled as a LOAD-PATH
# UNIT runs its target when the constant is READ, whichever spelling named it,
# and whether or not anything else ever requires it.
#
# A `$LOAD_PATH.unshift` of a computed directory makes zeo compile that whole
# directory as units, each with its own compiler and its own `autoload_consts`
# name set -- so the `autoload` here is seen only by demo.rb's compile while
# the read is emitted by the main program's, whose set is empty. What closes
# the gap is not sharing that set but widening the RESOLUTION: an `autoload`
# is a deferred `require`, so `run_pending_autoload` walks the same chain
# `Kernel#require` walks -- the compiled-in unit table, then the load path.
# Answering out of the unit table alone left a target the emitting compile
# never saw silently unloaded.

$LOAD_PATH.unshift(File.join(__dir__, "an_autoload_target_compiled_as_a_unit"))
require "demo"
require "spec"
p Demo::Thing::VALUE
p Demo::Spec.value
p Demo.autoload?(:Thing).nil?
