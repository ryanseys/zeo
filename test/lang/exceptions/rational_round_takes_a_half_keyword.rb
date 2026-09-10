# Rational#round accepts `half:` as :up, :even and :down, with and without a digit count.
r = (Rational(1,2).round(half: :up) rescue $!.class); p r
r2 = (Rational(5,2).round(0, half: :up) rescue $!.class); p r2
r3 = (Rational(5,2).round(0, half: :even) rescue $!.class); p r3
r4 = (Rational(5,2).round(0, half: :down) rescue $!.class); p r4
__END__
1
3
2
2
