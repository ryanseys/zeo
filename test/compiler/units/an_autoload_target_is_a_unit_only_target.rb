# An `autoload` target is a UNIT-ONLY target: every other require site that
# names the same file keeps its call and loads the unit, so whichever runs
# first runs the body once.
#
# Before that rule, a plain splice elsewhere claimed the dedup slot, no unit
# was built, and the constant read that should have run the target found
# nothing to run. `require "rubygems"` died on `Gem::Requirement` for exactly
# this reason: specification.rb reaches requirement.rb through a method-body
# `require_relative` that zeo splices at the file tail, BELOW the class body
# that reads the constant.
#
# The read also has to ASK. A compiled-in unit's classes are in the dispatch
# tables from program start, so the read never misses and no `const_missing`
# hook can carry the load -- the emitter gates the read itself.
require_relative "an_autoload_target_is_a_unit_only_target/lib"
require_relative "an_autoload_target_is_a_unit_only_target/spec"

# Reading the constant is what runs the target.
p Store::Rule::LIMIT
p Store::Rule.limit
p Store::Spec.limit
# ... and once it has run, ruby reports no pending autoload.
p Store.autoload?(:Rule).nil?
# The other site still works, and runs nothing a second time.
p Store.late
__END__
42
42
42
true
true
