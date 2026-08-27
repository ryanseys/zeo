# `class Bench < Probe` inside `module Mini` names `Mini::Probe`, even when an
# unrelated top-level `module Probe` is registered and `Mini::Probe` is not yet.
#
# A superclass clause names a CLASS, so a module found in an OUTER scope is not
# what the clause means and the search continues. Stopping on it refused the
# whole definition -- `superclass must be an instance of Class (given an
# instance of Module)` -- and left the subclass registered with no body site,
# which is a class the program defines and no read can find.
#
# minitest and test-unit are the real pair: `class Benchmark < Test` inside
# `module Minitest` found test-unit's top-level `module Test` whenever
# `minitest/test.rb` had not been walked yet, and which of the two units the
# compiler walks first is a demand-order fact, not a program fact.
#
# The rule is narrow on purpose. Preferring the inner scope OUTRIGHT is wrong:
# rspec-mocks writes `class BasicObject` under an `unless
# defined?(BasicObject)` that never runs, and the `class FluentInterfaceProxy <
# BasicObject` beneath it means the BUILTIN. An outer candidate that is already
# a class stays the answer.

require_relative "a_superclass_clause_skips_an_outer_module/outer"
require_relative "a_superclass_clause_skips_an_outer_module/root"

p Mini::Bench.superclass
p Mini::Bench.new.kind
p Mini::Bench.new.bench
p Mini::Bench.ancestors.take(2).map(&:to_s)
p Probe.class
p Mini::Probe.class
# An outer candidate that IS a class still wins -- the builtin, not a
# same-named class the program only defines under a guard that never runs.
module Guarded
  unless defined?(BasicObject)
    class BasicObject
    end
  end
  class Fluent < BasicObject
  end
end
p Guarded::Fluent.superclass.to_s
