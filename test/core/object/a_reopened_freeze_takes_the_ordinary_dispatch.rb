# A reopen of `String#freeze` takes the ordinary dispatch, so the
# literal-freeze fold stands down.
#
# It lives in a file of its own because the gate is a WHOLE-PROGRAM fact:
# zeo decides once per compile, where CRuby checks its redefinition flag when
# the instruction runs. Putting this reopen at the foot of
# `a_frozen_string_literal_is_not_deduplicated.rb` would therefore change
# every probe above it, which is the divergence rather than the rule.
#
# The gate has to ask TWO questions. `runtime_patches` records a RUNTIME
# redefinition -- `define_method(:freeze)` and friends -- and a compile-time
# builtin reopen is NOT one, as `a_later_def_on_a_builtin_reaches_back`
# records. Asking only the first kept the fold here and answered the literal
# where ruby answers the reopened body: a silently wrong program, not a
# slower one.
class String
  def freeze = "reopened:#{self}"
end
p "lit".freeze
p "lit".freeze.equal?("lit".freeze)
p "other".freeze

# The reopened body is an ordinary method, so `-@` is untouched by it.
p (-"lit").equal?(-"lit")
p (-"lit").frozen?
__END__
"reopened:lit"
false
"reopened:other"
true
true
