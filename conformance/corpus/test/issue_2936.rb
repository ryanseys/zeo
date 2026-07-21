# A sum block with a leading statement (a block-local) feeding a nested
# Array#find: the leading statement must run, not be dropped.
UNITS = [["d", 86400], ["h", 3600], ["m", 60], ["s", 1]]
p ["1h", "30m"].sum { |t| label = t[-1]; UNITS.find { |l, _| l == label }[1] }
# leading statement in an integer sum block
p [1, 2, 3].sum { |x| y = x * 2; y }
# leading statement in a float sum block
p [1.0, 2.0].sum { |x| z = x + 1.0; z }
