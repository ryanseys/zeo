# An Integer or Float operation with a nil argument raises TypeError, for
# coerce, pow, fdiv, divmod, round and digits alike.
#
r001 = (1.5.coerce(nil) rescue $!.class)
p r001

r002 = (5.pow(nil) rescue $!.class); p r002
r003 = (5.fdiv(nil) rescue $!.class); p r003
r004 = (5.divmod(nil) rescue $!.class); p r004
r005 = (5.round(nil) rescue $!.class); p r005
r006 = (5.digits(nil) rescue $!.class); p r006
r007 = (5.to_s(nil) rescue $!.class); p r007
r008 = (5.coerce(nil) rescue $!.class); p r008
r009 = (5[nil] rescue $!.class); p r009
r010 = (5.gcdlcm(nil) rescue $!.class); p r010
r011 = (5.div(nil) rescue $!.class); p r011
r012 = (5.modulo(nil) rescue $!.class); p r012
r013 = (5.remainder(nil) rescue $!.class); p r013
r014 = (5.ceildiv(nil) rescue $!.class); p r014
r015 = (1.5.fdiv(nil) rescue $!.class); p r015
r016 = (1.5.divmod(nil) rescue $!.class); p r016
r017 = (1.5.round(nil) rescue $!.class); p r017

r018 = (5.between?(nil, 9) rescue $!.class); p r018

r019 = (1.5 + nil rescue $!.class); p r019    # => TypeError
r023 = (1.5 ** nil rescue $!.class); p r023   # => TypeError
r024 = (1.5 % nil rescue $!.class); p r024    # => TypeError
r025 = (5.gcd(nil) rescue $!.class); p r025   # => TypeError
r026 = (5.lcm(nil) rescue $!.class); p r026   # => TypeError
r027 = (5 + nil rescue $!.class); p r027      # => TypeError
r028 = (5 < nil rescue $!.class); p r028      # => ArgumentError
__END__
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
ArgumentError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
ArgumentError
