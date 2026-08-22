# `Integer <=> Float` is decided EXACTLY (`rb_integer_float_cmp`), and an
# incomparable pair answers FALSE rather than raising.
#
# Both halves were wrong. Promoting the Integer to a double and comparing
# there answers `10**100 == 1.0e100` true -- they are different numbers and
# the double cannot tell -- and the same promotion made `2**53 + 1` equal to
# `2.0**53`, which is the shape that reaches ordinary code. The emitter's
# inline `fcmp` for a mixed pair now gives up past 2**53 and lets the
# runtime compare in integers.
#
# The NaN half: CRuby hand-writes `<`/`<=`/`>`/`>=` on Integer and Float so
# that `<=>`'s nil becomes false. Only a Rational or Complex RECEIVER keeps
# Comparable's `comparison of X with Y failed` -- which is why
# `Float::NAN < Rational(1,2)` is false while `Rational(1,2) < Float::NAN`
# raises.
big = 10**100
p [1.0e100 == big, big == 1.0e100, big.to_f == big, (2**70).to_f == 2**70]
p [2**53 + 1 == 2.0**53, 2.0**53 == 2**53 + 1, 2**53 + 1 > 2.0**53, 2.0**53 < 2**53 + 1]
p [(2**53 + 1) <=> 2.0**53, 2.0**53 <=> (2**53 + 1), 2**53 <=> 2.0**53]
p [big <=> 1.0e100, 1.0e100 <=> big, big > 1.0e100, 1.0 / 0 > big]

nan = Float::NAN
p [1 < nan, 1 > nan, 1 <= nan, 1 >= nan, 1 == nan]
p [big < nan, big > nan, big <= nan, big >= nan]
p [nan < 1, nan > 1, nan < big, nan > big, nan < nan]
p [nan < Rational(1, 2), nan > Rational(1, 2)]
begin
  Rational(1, 2) < nan
rescue ArgumentError => e
  p e.message
end
begin
  1 < "x"
rescue ArgumentError => e
  p e.message
end
