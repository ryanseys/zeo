# Rational: literals, reduction, exact arithmetic across the Int lane,
# `2 ** -2`, `quo`, Float promotion, rounding family, and the
# ZeroDivisionError channel.

r = 3r
puts r.class
p r
puts r
p 1.5r
p Rational(4, 8)
p Rational(1, 2) + Rational(1, 3)
p Rational(1, 2) * 3
p Rational(1, 2) / Rational(3, 4)
p Rational(3, 4) ** 2
p 2 ** -2
p 1.quo(3)
p Rational(1, 2) + 0.5
puts Rational(1, 2) < Rational(2, 3)
puts Rational(1, 2) == 0.5
puts Rational(1, 3).to_f
puts Rational(7, 2).to_i
p Rational(-7, 2).floor
p Rational(-7, 2).ceil
p Rational(7, 2).round
p Rational(3, 4).numerator
p Rational(3, 4).denominator
begin
  Rational(1, 0)
rescue ZeroDivisionError => e
  puts "ZeroDivisionError: #{e.message}"
end
__END__
Rational
(3/1)
3/1
(3/2)
(1/2)
(5/6)
(3/2)
(2/3)
(9/16)
(1/4)
(1/3)
1.0
true
true
0.3333333333333333
3
-4
-3
4
3
4
ZeroDivisionError: divided by 0
