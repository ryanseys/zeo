# A require under a runtime-undecidable guard runs only when the guard
# passes -- CRuby's order of events. The false direction: this env var is
# never set, so `loud.rb`'s side effect must not appear (an eager splice ran
# it in every compile -- the divergence minitest's hell.rb recorded). The
# true direction: HOME is set in any test environment, so `second.rb` loads
# at runtime through its compiled-in unit.
require_relative "conditional_require_units/loud" if ENV["ZEO_TEST_NEVER_SET"]
puts "between"
require_relative "conditional_require_units/second" if ENV["HOME"]
puts "after"
