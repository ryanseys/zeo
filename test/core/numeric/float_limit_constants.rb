# Issue #1253: Float::MAX / MIN / EPSILON compiled to undeclared C
# identifiers (Float_MAX) instead of the <float.h> DBL_* macros.
# Issue #1252: Integer has no public constants in CRuby (it is
# arbitrary precision), so Integer::MAX must raise NameError, not emit
# a bare Integer_MAX identifier.
puts Float::MAX
puts Float::MIN
puts Float::EPSILON
puts(Float::MAX > Float::MIN)
puts((1.0 + Float::EPSILON) > 1.0)
imax = Integer::MAX rescue "NameError"
puts imax
imin = Integer::MIN rescue "NameError"
puts imin
__END__
1.7976931348623157e+308
2.2250738585072014e-308
2.220446049250313e-16
true
true
NameError
NameError
