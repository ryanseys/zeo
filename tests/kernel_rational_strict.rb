r001 = (Rational("bad") rescue $!.class); p r001
r002 = (Rational("") rescue $!.class); p r002
r003 = (Rational(nil) rescue $!.class); p r003
r004 = (Rational("1/2/3") rescue $!.class); p r004
p(Rational("1.5e2"))
p(Rational("2", "4") == Rational(1, 2))
p(Rational("1/3"))
p(Rational("0.5"))
p(Rational(1, 2))
p("bad".to_r)
