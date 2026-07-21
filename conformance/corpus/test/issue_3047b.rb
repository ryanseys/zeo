r = (Rational(1,2).round(half: :up) rescue $!.class); p r
r2 = (Rational(5,2).round(0, half: :up) rescue $!.class); p r2
r3 = (Rational(5,2).round(0, half: :even) rescue $!.class); p r3
r4 = (Rational(5,2).round(0, half: :down) rescue $!.class); p r4
