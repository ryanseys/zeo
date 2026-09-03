# `Enumerable#lazy` returns an Enumerator::Lazy: a chain of transformations
# that only runs on demand, so it can work over INFINITE sequences.

# Only three elements are ever computed, even though the source is unbounded.
p (1..Float::INFINITY).lazy.map { |x| x * 2 }.first(3)          # [2, 4, 6]
p (1..Float::INFINITY).lazy.select(&:even?).first(3)            # [2, 4, 6]

# Stages compose left to right and stay lazy until a terminal call.
p (1..Float::INFINITY).lazy.select(&:even?).map { |x| x * x }.first(3)  # [4, 16, 36]

# `take_while` bounds an infinite source; `to_a`/`force` then materializes it.
p (1..Float::INFINITY).lazy.map { |x| x * x }.take_while { |x| x < 30 }.to_a  # [1, 4, 9, 16, 25]

# take / drop / flat_map / uniq / compact / grep all have lazy forms.
p (1..Float::INFINITY).lazy.take(5).to_a                        # [1, 2, 3, 4, 5]
p [1, 2, 3, 4, 5, 6].lazy.drop(2).first(2)                      # [3, 4]
p (1..Float::INFINITY).lazy.flat_map { |x| [x, -x] }.first(4)   # [1, -1, 2, -2]
p [1, 1, 2, 3, 3, 1].lazy.uniq.to_a                             # [1, 2, 3]
p [1, nil, 2, nil, 3].lazy.compact.to_a                         # [1, 2, 3]
p (1..20).lazy.grep(5..10).to_a                                 # [5, 6, 7, 8, 9, 10]

# A finite lazy is an ordinary enumerable once forced.
p [1, 2, 3, 4].lazy.map { |x| x * 10 }.to_a                     # [10, 20, 30, 40]
p [1, 2, 3].lazy.class.name                                     # "Enumerator::Lazy"
__END__
[2, 4, 6]
[2, 4, 6]
[4, 16, 36]
[1, 4, 9, 16, 25]
[1, 2, 3, 4, 5]
[3, 4]
[1, -1, 2, -2]
[1, 2, 3]
[1, 2, 3]
[5, 6, 7, 8, 9, 10]
[10, 20, 30, 40]
"Enumerator::Lazy"
