# Rational and Complex let Numeric own the rows ruby gives Numeric, so
# `owner` and `instance_methods(false)` agree with ruby.
r = Rational(7, 2)
p r % 2, r.modulo(2.0), r.div(2), r.div(0.5), +r, r.i, r.to_int
p r.finite?, r.infinite?, Rational(-7, 2).to_int
p Complex(3, 0).to_int, +Complex(1, 2)
p Rational.instance_method(:%).owner, Complex.instance_method(:+@).owner
p (Rational.instance_methods(false) & %i[% modulo div finite? infinite? i to_int +@])
p (Complex.instance_methods(false) & %i[to_int +@])
begin
  Complex(1, 2).to_int
rescue StandardError => e
  p e
end
begin
  r % 0
rescue StandardError => e
  p e
end
begin
  r.div("a")
rescue StandardError => e
  p e
end
__END__
(3/2)
1.5
1
7
(7/2)
(0+(7/2)*i)
3
true
nil
-3
3
(1+2i)
Numeric
Numeric
[]
[]
#<RangeError: can't convert 1+2i into Integer>
#<ZeroDivisionError: divided by 0>
#<TypeError: String can't be coerced into Rational>
