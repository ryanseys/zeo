# A unit reached through `autoload` registers as loaded but its BODY never
# runs, so every constant it assigns stays undefined.
#
# rack is the case that found it: `rack.rb` declares
# `autoload :Utils, "rack/utils"`, and `rack/utils.rb` opens with
# `require "time"`. After touching `Rack::Utils` both files appear in
# `$LOADED_FEATURES` -- so the loader thinks it ran them -- and yet
# `Time::VERSION`, assigned on the second line of `class Time`, raises
# `NameError`. The fixture here is that shape with nothing vendored: the
# autoload target requires a LEAF, and the leaf's body is what never runs.
#
# The class-body call for a lazily-demanded single unit is what does not
# happen. `loader.rs`'s `single_unit_demand` path records the unit's constants
# in `unrun_unit_consts` and notes that "a unit's classes register at startup"
# -- registration is not execution, and nothing runs the body when the
# autoload target loads.
#
# PRE-EXISTING, and invisible until the builtin-reopen flag made it visible: a
# reopen's rows registered at startup either way, so `Time#httpdate` answered
# whether or not the body defining it ever ran. That is why
# `collect_reopen_flags` skips a site in a lazy unit -- until this is fixed a
# flag there would stay zero for the whole program, and a working reopen would
# become `undefined method`.

module Holder
  autoload :Target, "#{__dir__}/an_autoloaded_units_body_runs/target"
end

p defined?(LEAF_RAN)
p Holder::Target.go
p defined?(LEAF_RAN)
p [].leaf_method
