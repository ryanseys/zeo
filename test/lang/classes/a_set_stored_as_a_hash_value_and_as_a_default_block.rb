# Appending to a Set held in a hash, and to one a default block creates per
# key.
# (spinel issue #3174)
require 'set'
h = { a: Set.new }
h[:a] << 1
p h[:a].to_a
adj = Hash.new { |hh, k| hh[k] = Set.new }
adj[:x] << 1
adj[:x] << 2
adj[:y] << 3
p adj[:x].to_a.sort
p adj[:y].to_a
__END__
[1]
[1, 2]
[3]
