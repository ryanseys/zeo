# Complex() and Rational() with `exception: false` answer nil for a value
# they cannot convert.
#
p(Complex("bad", exception: false))

p(Rational("x", exception: false))
# Ruby: nil   Spinel: invalid value for Rational(): "x" (ArgumentError)
c = Complex("bad", exception: false); p c
# Ruby: nil   Spinel: invalid value for convert():  (ArgumentError)

p(Complex("1+2i", exception: false))    # => (1+2i)
p(Rational("1/2", exception: false))    # => (1/2)
p(Integer("abc", exception: false))     # => nil
p(Integer("ff", 16, exception: false))  # => 255
p(Float("x", exception: false))         # => nil
__END__
nil
nil
nil
(1+2i)
(1/2)
nil
255
nil
