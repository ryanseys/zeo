# Kernel#Rational and Kernel#Complex parse a String argument as a numeric
# literal, rather than reading the string's pointer as an integer.

# Rational("n/d" | "n.d" | "n"): the DECIMAL value, so "2.5" is exactly 5/2
# (not the Float value). Whitespace and a sign are allowed; a zero denominator
# raises ZeroDivisionError, an unparseable string ArgumentError.
p Rational("3/4")
p Rational("  -5/2 ")
p Rational("6")
p Rational("2.5")
p Rational(3)          # the numeric form is unchanged

# Complex("a+bi"): each component is an Integer without a decimal point, else a
# Float; a bare "±i" is the unit, a pure real or pure imaginary is supported.
p Complex("2+3i")
p Complex("1+2i")
p Complex("3")
p Complex("-i")
p Complex("4i")
p Complex("1.5-2.5i")

def caught
  yield
rescue => e
  "#{e.class}: #{e.message}"
end
p caught { Rational("abc") }
p caught { Complex("xyz") }
p caught { Rational(1, 0) }
