# Issue #404 Phase 3 Tier 4 follow-up: `case obj when ClassConst`
# on a poly receiver. Pre-Tier-4 the lowering left ConstantReadNode
# arms unimplemented for poly recv (literal cascade only handled
# sym/str/int/float/nil/bool). Module#=== is equivalent to
# `arg.is_a?(recv)`, so we route through sp_class_le over
# sp_class_for_poly and the precomputed ancestors.
#
# Coverage:
#   - Class match against a poly recv carrying primitives.
#   - Multiple primitive class arms in one case.
#   - else clause.

def describe(v)
  case v
  when Integer
    "int"
  when String
    "string"
  when Float
    "float"
  when Symbol
    "symbol"
  else
    "other"
  end
end

puts describe(42)
puts describe("hello")
puts describe(3.14)
puts describe(:sym)
puts describe(nil)
