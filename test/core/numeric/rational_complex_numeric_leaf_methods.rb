r = Rational(3, 2)
p r.finite?
p r.infinite?
p r.coerce(2)
p r.coerce(2.0)
p r.div(1)
p Rational(7, 2).div(2)
p r.i
c = Complex(6, 0)
p c.finite?
p c.infinite?
p c.to_f
p c.to_i
p c.to_r
p c.coerce(3)
p Complex(3, 4).finite?
p Complex(3, 4).numerator
p Complex(3, 4).denominator
p Complex(Rational(2, 3), Rational(3, 4)).numerator
p Complex(Rational(2, 3), Rational(3, 4)).denominator
begin
  Complex(3, 4).to_f
rescue RangeError => e
  puts e.message
end
__END__
true
nil
[(2/1), (3/2)]
[2.0, 1.5]
1
1
(0+(3/2)*i)
true
nil
6.0
6
(6/1)
[(3+0i), (6+0i)]
true
(3+4i)
1
(8+9i)
12
can't convert 3+4i into Float
