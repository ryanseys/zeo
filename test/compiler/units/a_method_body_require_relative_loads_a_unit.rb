# A `require_relative` written in a METHOD body loads its target when the
# method RUNS, even when some other site has already made that file a unit.
#
# The guard below makes `part.rb` a unit-only target: it is never spliced, and
# every site naming it keeps its call so that whichever one runs first loads
# the body once. The method-body site is the only one that ever runs here.
#
# It used to load nothing. `RequireCollector` files a method-body
# `require_relative` under `lazy`, never `calls`, so the statement pre-pass --
# the one place that consults `unit_only_targets` before lowering -- never saw
# it. `lower_node` reached the `def` first and folded the call to `true`, and
# the trailing pass that would have claimed the site ran long after. The call
# was dead and the file never loaded.
#
# Both sites now claim through one helper, `Loader::claim_unit_only_site`. The
# two arrive at different times, which is the whole bug: a statement-position
# require claims itself as it lowers, and a method-body one has to claim
# itself in the pre-pass, ahead of the fold.

require_relative "a_method_body_require_relative_loads_a_unit/part" if ENV["NEVER"]

def load_part
  require_relative "a_method_body_require_relative_loads_a_unit/part"
end

p defined?(PART)
p load_part
p defined?(PART)
p PART
# A second call answers false: the feature is loaded.
p load_part
__END__
nil
true
"constant"
"loaded"
false
