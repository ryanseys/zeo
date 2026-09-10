# The leading assignment runs, and a nested Array#find reads it; also in an
# Integer sum.
UNITS = [["d", 86400], ["h", 3600], ["m", 60], ["s", 1]]
p ["1h", "30m"].sum { |t| label = t[-1]; UNITS.find { |l, _| l == label }[1] }
# leading statement in an integer sum block
p [1, 2, 3].sum { |x| y = x * 2; y }
# leading statement in a float sum block
p [1.0, 2.0].sum { |x| z = x + 1.0; z }
__END__
3660
12
5.0
