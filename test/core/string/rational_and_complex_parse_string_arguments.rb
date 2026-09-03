p Rational("3/4")
p Rational("  -5/2 ")
p Rational("6")
p Rational("2.5")
p Rational(3)
p Complex("2+3i")
p Complex("1+2i")
p Complex("3")
p Complex("-i")
p Complex("4i")
def t; yield; rescue => e; "#{e.class}"; end
p t { Rational("abc") }
p t { Complex("xyz") }
p t { Rational(1, 0) }
__END__
(3/4)
(-5/2)
(6/1)
(5/2)
(3/1)
(2+3i)
(1+2i)
(3+0i)
(0-1i)
(0+4i)
"ArgumentError"
"ArgumentError"
"ZeroDivisionError"
