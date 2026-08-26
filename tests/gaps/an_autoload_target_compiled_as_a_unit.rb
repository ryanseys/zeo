# An `autoload` written inside a file that is itself compiled as a LOAD-PATH
# UNIT never runs its target: the constant read raises where ruby loads the
# file.
#
# THE NARROW REMAINDER of RubyGems blocker 1, which is otherwise FIXED --
# `require "rubygems"` works, and `tests/an_autoload_target_is_a_unit_only_
# target.rb` pins the shape that mattered. Two things had to change for that:
# an autoload target became a UNIT-ONLY target (so a plain splice elsewhere
# no longer claims its slot), and the emitter learned to gate a read that
# resolves to no compile-time class.
#
# WHAT IS STILL BROKEN, measured 2026-08-26. A `$LOAD_PATH.unshift` of a
# computed directory makes zeo compile that whole directory as UNITS, each
# with its OWN compiler and its own `autoload_consts` name set. The
# `autoload` here is seen by demo.rb's unit compile; the read is emitted by
# the MAIN program's compile, whose set is empty -- so the read is folded to
# a Class immediate with no touch beside it. The two runtime-lookup arms ask
# unconditionally now and are fine; the compile-time FOLD cannot, because
# emitting a touch beside every class-immediate read would tax the hot path.
#
# WHERE THE NEXT ATTEMPT SHOULD START. The name set has to be shared across
# the compiles of one program, or the fold has to know that the class it
# resolved belongs to a unit that has not run. The second is the narrower
# question and the compiler already tracks `LoadedFile::is_unit`.
#
# Ruby's answer: reading the constant loads the file, whichever spelling
# named it, and whether or not anything else ever requires it.

$LOAD_PATH.unshift(File.join(__dir__, "an_autoload_target_compiled_as_a_unit"))
require "demo"
require "spec"
p Demo::Thing::VALUE
p Demo::Spec.value
p Demo.autoload?(:Thing).nil?
