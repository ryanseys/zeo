# Correctness fixes: Time.utc/local accept a fractional-microsecond 7th
# argument (Float/Rational) and keep the sub-microsecond nanoseconds;
# Rational#round honors the `half:` keyword (:up/:even/:down); Float
# round/truncate with an extreme ndigits stays finite instead of NaN; and
# Enumerator.new(callable).size invokes the callable lazily. Byte-verified
# against ruby 4.0.6.

p Time.utc(2001, 2, 3, 4, 5, 6, 500.5).nsec
p Time.utc(2020, 1, 1, 0, 0, 0, Rational(1, 2)).nsec
p Rational(5, 2).round(half: :even)
p Rational(5, 2).round(half: :down)
p Rational(5, 2).round(half: :up)
p Rational(-5, 2).round(half: :even)
p 1.23.round(400)
p 1.23.round(-400)
p 2.5.truncate(1000)
p Enumerator.new(lambda { 42 }) { |y| y << 1 }.size
p Enumerator.new(5) { |y| y << 1 }.size
p Enumerator.new { |y| y << 1 }.size
__END__
500500
500
2
2
3
-2
1.23
0
2.5
42
5
nil
