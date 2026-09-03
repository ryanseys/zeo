# A Rational/Float seconds field splits into the integer second plus an
# EXACT sub-second (Time stores a rational epoch, so #subsec is exact).

p Time.utc(2020, 1, 1, 0, 0, Rational(3, 2)).subsec
p Time.utc(2020, 1, 1, 0, 0, Rational(3, 2)).sec
p Time.utc(2020, 1, 1, 0, 0, 2.5).subsec
__END__
(1/2)
1
(1/2)
