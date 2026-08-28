# sort, min and max with a comparator block of two, one and zero
# parameters, on Integer, Float and String arrays. CRuby generated the
# expectations.
p [1, 5, 3].max { |x| -x }
p [3, 1, 2].sort { |x| 0 }
p [3, 1, 2].sort { 0 }
p [3, 1, 2].min { |x| 1 }
p [3, 1, 2].sort { |a, b| b <=> a }
p [3, 1, 2].max { |a, b| a <=> b }
p %w[bb a ccc].sort { |x| x.length }
p [2.5, 1.5].min { |x| -1 }
