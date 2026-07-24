# `expr rescue fallback` as an rvalue — verifies that
# RescueModifierNode is wired into infer_type so the receiving
# variable gets the unified type rather than the default int.
#
# Note: Ruby's `=` has higher precedence than the rescue modifier,
# so `a = "x" rescue "y"` parses as `(a = "x") rescue "y"` — the
# RescueModifierNode wraps the assignment, not the rvalue. To
# actually exercise the RescueModifierNode-as-rvalue path through
# infer_type, the rescue must be parenthesized as a sub-expression.

# Same-type branches: string=string. Trivial baseline.
a = ("ok" rescue "fallback")
puts a

# Mismatched branches: main is `raise` (returns int 0 from
# compile_expr but never actually returns), fallback is string.
# The fallback wins type inference; result type is string.
b = (raise "boom" rescue "default")
puts b

# Same-type int. Endless method shape stays unchanged.
def parse_int(s) = Integer(s) rescue 0
puts parse_int("42")
puts parse_int("abc")
