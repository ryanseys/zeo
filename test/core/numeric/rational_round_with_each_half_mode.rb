# :even, :up and :down, the bare form, and a negative Rational.
# (spinel issue #3047)
p Rational(5, 2).round(half: :even)
p Rational(5, 2).round(half: :up)
p Rational(5, 2).round(half: :down)
p Rational(5, 2).round
p Rational(7, 2).round(half: :even)
p Rational(-5, 2).round(half: :even)
p Rational(-5, 2).round(half: :down)
p Rational(3, 2).round(half: :even)
__END__
2
3
2
3
4
-2
-2
2
