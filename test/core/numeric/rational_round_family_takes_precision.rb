# floor/ceil/truncate WITH a precision keep the fraction: 157/50 is
# 3.14, so floor(1)=3.1=(31/10), ceil(1)=3.2=(16/5), truncate(1)=(31/10)
# (verified against ruby 4.0.6 -- the previous expectation wrongly kept
# (157/50) for all three).

r = Rational(157, 50)
p r.round(2)
p r.round(-1)
p r.round
p r.floor(1)
p r.ceil(1)
p r.truncate(1)
p Rational(-7, 2).round
__END__
(157/50)
0
3
(31/10)
(16/5)
(31/10)
-4
