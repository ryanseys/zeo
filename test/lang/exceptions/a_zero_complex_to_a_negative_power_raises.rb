# Complex(0, 0) raised to a negative power is a ZeroDivisionError; a positive power is fine.
p((Complex(0, 0) ** -1 rescue $!.class))
p((Complex(0, 0) ** -2 rescue $!.class))
p(Complex(0, 0) ** 2)
__END__
ZeroDivisionError
ZeroDivisionError
(0+0i)
