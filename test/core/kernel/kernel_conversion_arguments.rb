# Kernel conversion functions raise TypeError for an argument with no
# conversion, and consult to_f/to_int where there is one.
#
r001 = (Integer({}) rescue $!.class)
p r001

class Conv003; def to_f; 1.25; end; end
p(Float(Conv003.new))          # Ruby: 1.25
p(Float(Rational(1, 2)))       # Ruby: 0.5
p(Float(Complex(1.5, 0)))      # Ruby: 1.5

class Conv004; def to_int; 8; end; end
p(Integer(Conv004.new))        # Ruby: 8
p(Integer(Rational(4, 2)))     # Ruby: 2
p(Integer(Complex(3, 0)))      # Ruby: 3

r005 = (Float(true) rescue $!.class); p r005      # Ruby: TypeError
r006 = (Float(:s) rescue $!.class); p r006        # Ruby: TypeError

r007 = (Integer([1]) rescue $!.class); p r007     # => TypeError
r008 = (Integer(1..2) rescue $!.class); p r008    # => TypeError
r009 = (Integer(true) rescue $!.class); p r009    # => TypeError
r010 = (Integer(false) rescue $!.class); p r010   # => TypeError
r011 = (Integer(:sym) rescue $!.class); p r011    # => TypeError

p(Integer("42"))
p(Float("1.5"))
p(Integer(3.9))
p(Float(3))
r = (Float(Complex(1, 2)) rescue $!.class); p r
r = (Integer(Complex(1, 2)) rescue $!.class); p r
r = (Integer({}) rescue $!.class); p r
r = (Float([]) rescue $!.class); p r
__END__
TypeError
1.25
0.5
1.5
8
2
3
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
42
1.5
3
3.0
RangeError
RangeError
TypeError
TypeError
