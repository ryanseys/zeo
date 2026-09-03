# A `require` written inside a METHOD body defines its feature's constants
# when the method RUNS, exactly as CRuby does -- not before the program's
# first statement.
#
# The file is compiled in as a UNIT: a load-path file nothing has required
# yet. Its classes register for dispatch at startup because the static MRO
# needs a shape, so `PrettyPrint` used to answer `defined?` before anything
# loaded it.
#
# The fix reuses the machinery a runtime-CONDITIONAL class already had. Such
# a class registers its shape and starts CONCEALED, revealed at the head of
# its own body site; a unit's body site lives in the unit's function, so it
# runs when the unit does. `Compiler::class_waits_for_its_unit` is the
# predicate, and it feeds the same three places: `clif::classes`'s conceal
# list, the class-body site's reveal, and `constant_is_positional`, which is
# what stops `defined?` folding an answer at compile time.
#
# A VALUE constant a unit assigns needs the other half:
# `Hir::unrun_unit_consts` records every unit's constant names -- it used to
# record only an autoload target's -- and a bare-name `defined?` of one asks
# the run time instead of concluding "written only later". A unit is not in
# any statement stream, so "later" and "in a file nothing has run" looked
# alike and meant opposite things.
#
# Reading a concealed constant also RUNS a pending `autoload` now. That is
# where ruby loads, and a concealed class is exactly the shape an autoload
# targets: registered for dispatch, no constant yet.
#
# The size half landed separately: `narrow_runtime_eval` drops a plain
# `require` whose LITERAL feature names a unit this compile emitted, because
# `dynamic_require` asks `features::load_feature` before the on-disk tier
# that needs a compiler. `require_relative` and `load` stay conservative --
# the first resolves against the calling file's directory, which is a
# run-time fact, and the second asks DISK first.

def lazy
  require "prettyprint"
end

p defined?(PrettyPrint)
p lazy
p defined?(PrettyPrint)
p lazy
__END__
nil
true
"constant"
false
