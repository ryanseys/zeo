# Each materializes the pairs.
h = [{ 20 => 2, 3 => 1 }].first
p h.sort
p h.min
p h.max
p h.sort_by { |k, v| v }
p h.sort_by { |k, v| k }
__END__
[[3, 1], [20, 2]]
[3, 1]
[20, 2]
[[3, 1], [20, 2]]
[[3, 1], [20, 2]]
