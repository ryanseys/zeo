# Float#remainder by zero raises ZeroDivisionError, for a Float or an
# Integer divisor and through a local.
#
r001 = (7.0.remainder(0.0) rescue $!.class); p r001
r002 = (7.0.remainder(0) rescue $!.class); p r002
v003 = 7.0; w003 = 0.0; r003 = (v003.remainder(w003) rescue $!.class); p r003
r004 = (0.0.remainder(0.0) rescue $!.class); p r004
r005 = (7.remainder(0.0) rescue $!.class); p r005
p(7.0.remainder(2.0))
p(7.remainder(2.5))
__END__
ZeroDivisionError
ZeroDivisionError
ZeroDivisionError
ZeroDivisionError
ZeroDivisionError
1.0
2.0
