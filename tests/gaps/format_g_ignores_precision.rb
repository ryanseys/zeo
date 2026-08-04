# `%g` picks between `%e` and `%f` and shows a fixed number of SIGNIFICANT
# digits -- six by default, `%.Ng` for N, and `%#g` keeps the trailing zeros
# that plain `%g` strips. zeo ignores the precision entirely and prints the
# shortest representation that round-trips:
#
#     format("%g", 123456789.0)     ruby "1.23457e+08"   zeo "1.23456789e+08"
#     format("%.3g", 123456789.0)   ruby "1.23e+08"      zeo "1.23456789e+08"
#     format("%.10g", 123456789.0)  ruby "123456789"     zeo "1.23456789e+08"
#     format("%#g", 1.5)            ruby "1.50000"       zeo "1.5"
#
# `%g` is the directive for "show me this number readably", so it is what a
# report, a table or a log line uses -- and a shortest-round-trip answer defeats
# the point of asking for a fixed width. The choice of `%e` vs `%f` also
# depends on the precision (ruby switches when the exponent is < -4 or >= the
# precision), which is why `%.10g` above comes out positional in ruby and
# scientific in zeo.
#
# `%e` and `%f` already honour their precision, so only `%g`/`%G` diverge.

p [123456789.0, 1234.5678, 0.000012345678, 1.0, 100.0, 0.0].map { format("%g", _1) }
p [format("%.3g", 123456789.0), format("%.10g", 123456789.0), format("%.1g", 1234.0)]
p format("%G", 123456789.0)
p [format("%g", 1.500), format("%#g", 1.5)]

# These already agree.
p [format("%e", 123456789.0), format("%.2e", 1234.5), format("%f", 1.5), format("%.2f", 1.5)]
