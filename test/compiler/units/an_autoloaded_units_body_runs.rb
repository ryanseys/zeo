# A unit reached through `autoload` runs its BODY, so every constant it
# assigns is defined from there on.
#
# rack is the case that found it: `rack.rb` declares
# `autoload :Utils, "rack/utils"`, and `rack/utils.rb` opens with
# `require "time"`. Both files appeared in `$LOADED_FEATURES` -- so the
# loader thought it had run them -- and yet `Time::VERSION`, assigned on the
# second line of `class Time`, raised `NameError`. The fixture here is that
# shape with nothing vendored: the autoload target requires a LEAF, and the
# leaf's constants are what never appeared.
#
# Registration was not execution, and the two were being read as one. A
# unit's classes register for dispatch at startup -- the static MRO needs a
# shape -- which made the constant answer before the file ran. Now a unit's
# class starts CONCEALED and its own body site reveals it, the same
# mechanism a runtime-conditional class uses; and `Hir::unrun_unit_consts`
# records every unit's VALUE constants, not just an autoload target's, so a
# bare-name `defined?` asks the run time rather than folding "written only
# later" -- a unit is in no statement stream, so later and never-run looked
# the same.
#
# See `a_deferred_require_loads_when_it_runs.rb` for the other half of the
# same cause.

module Holder
  autoload :Target, "#{__dir__}/an_autoloaded_units_body_runs/target"
end

p defined?(LEAF_RAN)
p Holder::Target.go
p defined?(LEAF_RAN)
p [].leaf_method
__END__
nil
:go
"constant"
:leaf_method
