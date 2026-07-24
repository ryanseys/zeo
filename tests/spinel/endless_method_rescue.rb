# Endless method with rescue modifier:
#   def name(args) = expr rescue fallback
# Combines `def foo(x) = expr` (already supported) with the postfix
# `rescue` modifier. This test
# pins int-typed-return shapes.

# Plain endless + rescue with Integer() conversion
def parse_int(s) = Integer(s) rescue 0
puts parse_int("42")
puts parse_int("abc")
puts parse_int("0")
puts parse_int("-7")

# Explicit-raise rescue trigger. Pre-fix the test was `half(n) = n / 2`,
# which never raises (0/2 == 0) and so never exercised the rescue path.
# Uses an explicit raise() so the rescue trigger is independent of the
# integer-division-by-zero machinery (covered separately in
# test/integer_div_by_zero.rb). The raise lives inside a helper so the
# endless body is a single call expression — keeps codegen happy and
# exercises the cross-frame rescue path.
def assert_pos(n)
  n < 0 ? raise("negative") : n
end
def safe(n) = assert_pos(n) rescue -1
puts safe(10)
puts safe(-5)
puts safe(0)

# Nested call with rescue
def chain(s) = s.to_i + 1 rescue 0
puts chain("99")
puts chain("zero")
puts chain("-1")
