# Float#<=> 3-way compare returns -1 / 0 / 1, mirroring Integer#<=>.
p(3.0 <=> 5.0)
p(5.0 <=> 3.0)
p(3.0 <=> 3.0)
p(3.5 <=> 3)
p(3 <=> 3.5)

# A float comparator block now drives sort / min / max / minmax.
p [3.5, 1.5, 2.5].sort { |a, b| a <=> b }
p [3.5, 1.5, 2.5].min { |a, b| a <=> b }
p [3.5, 1.5, 2.5].max { |a, b| a <=> b }
p [3.5, 1.5, 2.5].minmax { |a, b| a <=> b }
p [3.5, 1.5, 2.5].minmax { |a, b| b <=> a }
