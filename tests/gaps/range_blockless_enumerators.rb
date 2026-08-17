# GAP -- imported from the spinel corpus at c55d9bdb.
# A blockless enumerator over an endless Range is evaluated eagerly, so the
# program never terminates and the harness kills it at the 60s deadline.
#
r = ((1..5).cycle(2).to_a rescue $!.class)
p r

r = ((1..5).each_entry.to_a rescue $!.class); p r        # Ruby: [1, 2, 3, 4, 5]      Spinel: NoMethodError
r = ((1..).each_slice(2).first(2) rescue $!.class); p r  # Ruby: [[1, 2], [3, 4]]     Spinel: RangeError

r = ((1..).each_cons(2).first(2) rescue $!.class); p r        # Ruby: [[1, 2], [2, 3]]   Spinel: RangeError
r = ((1..).each_with_index.first(2) rescue $!.class); p r     # Ruby: [[1, 0], [2, 1]]   Spinel: NoMethodError

p((1..3).each_with_index.to_a)   # => [[1, 0], [2, 1], [3, 2]]

o = []; (1..3).cycle(2) { |x| o << x }; p o    # => [1, 2, 3, 1, 2, 3]
o = []; (1..3).each_entry { |x| o << x }; p o   # => [1, 2, 3]

r = ((1..).each_slice(3).first(1) rescue $!.class); p r
r = ((5..).each_with_index.first(3) rescue $!.class); p r
r = ((1..).lazy.with_index(10).first(2) rescue $!.class); p r
r = ([1, 2, 3, 4].each_cons(2).to_a rescue $!.class); p r
