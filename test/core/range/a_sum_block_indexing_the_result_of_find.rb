# The block folds through the sum rather than being dropped.
# (spinel issue #2925)
table = [["h", 3600], ["m", 60]]
p ["1h", "30m"].sum { |t| t[0...-1].to_i * table.find { |l, _| l == t[-1] }[1] }
__END__
5400
