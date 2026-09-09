# From the spinel corpus (c55d9bdb).
# Kernel#Integer / #Float: `exception: false` asks for nil rather than a raise,
# and the keyword hash is not one of the value arguments (passed through as one
# it landed in the base slot as a pointer). A value with no integer conversion
# is a TypeError, not the value reinterpreted.
p(Integer("abc", exception: false))
p(Integer("12", exception: false))
p(Integer("ff", 16, exception: false))
p(Integer("zz", 16, exception: false))
p(Float("abc", exception: false))
p(Float("1.5", exception: false))
p((Integer([1]) rescue $!.class))
p((Integer({ a: 1 }) rescue $!.class))
p((Integer(:s) rescue $!.class))
p((Integer(nil) rescue $!.class))

# the strict forms keep their behaviour
p(Integer("0x1f", 16))
p(Integer(" 42 "))
p((Integer("abc") rescue $!.class))
p(Float("1.5"))
p((Float("x") rescue $!.class))
p(Integer(3.9))
__END__
nil
12
255
nil
nil
1.5
TypeError
TypeError
TypeError
TypeError
31
42
ArgumentError
1.5
ArgumentError
3
