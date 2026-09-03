# `Array#sort` with a comparator block leaves equal keys wherever the C
# library puts them: `ruby_qsort` IS the system `qsort_r`, and zeo calls the
# same one, so the mixed-case word list below ("Apple"/"apple" compare equal)
# and the 60-element tie table at the end are the tie order this pins.
#
# This was a decided divergence while zeo sorted stably. Imported from the
# spinel corpus at c55d9bdb; see tests/spinel/UPSTREAM.md.
#
# TWO goldens, because the tie order is libc's and glibc's qsort is not BSD's:
# `.expected` is macOS, `.linux.expected` linux.
#
a = [5, 3, 9, 1, 3, 7, 0, 3, 8, 2]
p a.sort { |x, y| x <=> y }
p a.sort { |x, y| y <=> x }
p a
b = a.dup
b.sort! { |x, y| x <=> y }
p b
words = %w[pear Apple fig banana kiwi date apple Fig]
p words.sort { |x, y| x.downcase <=> y.downcase }
pairs = [[1, "a"], [0, "b"], [1, "c"], [0, "d"], [2, "e"], [1, "f"]]
p pairs.sort { |x, y| x[0] <=> y[0] }
f = [3.5, -1.0, 2.25, 0.0]
p f.sort { |x, y| x <=> y }
p [].sort { |x, y| x <=> y }
p [7].sort { |x, y| x <=> y }
p [2, 1].sort { |x, y| x <=> y }
h = { "b" => 2, "a" => 1, "c" => 3 }
p h.sort { |x, y| x[0] <=> y[0] }
objs = [{ "k" => 3 }, { "k" => 1 }, { "k" => 2 }]
p objs.sort { |x, y| x["k"] <=> y["k"] }.map { |h| h["k"] }

# a big enough input that a quadratic sort would be visible, checked by value
big = Array.new(2000) { |i| (i * 7919) % 2003 }
sorted = big.sort { |x, y| x <=> y }
p sorted == big.sort
p sorted.first, sorted.last

# stability: equal keys keep their original order
tagged = Array.new(60) { |i| [i % 3, i] }
p tagged.sort { |x, y| x[0] <=> y[0] }.first(6)
__END__
[0, 1, 2, 3, 3, 3, 5, 7, 8, 9]
[9, 8, 7, 5, 3, 3, 3, 2, 1, 0]
[5, 3, 9, 1, 3, 7, 0, 3, 8, 2]
[0, 1, 2, 3, 3, 3, 5, 7, 8, 9]
["apple", "Apple", "banana", "date", "fig", "Fig", "kiwi", "pear"]
[[0, "b"], [0, "d"], [1, "a"], [1, "c"], [1, "f"], [2, "e"]]
[-1.0, 0.0, 2.25, 3.5]
[]
[7]
[1, 2]
[["a", 1], ["b", 2], ["c", 3]]
[1, 2, 3]
true
0
2002
[[0, 3], [0, 21], [0, 0], [0, 39], [0, 24], [0, 9]]
#@ linux stdout
[0, 1, 2, 3, 3, 3, 5, 7, 8, 9]
[9, 8, 7, 5, 3, 3, 3, 2, 1, 0]
[5, 3, 9, 1, 3, 7, 0, 3, 8, 2]
[0, 1, 2, 3, 3, 3, 5, 7, 8, 9]
["Apple", "apple", "banana", "date", "fig", "Fig", "kiwi", "pear"]
[[0, "b"], [0, "d"], [1, "a"], [1, "c"], [1, "f"], [2, "e"]]
[-1.0, 0.0, 2.25, 3.5]
[]
[7]
[1, 2]
[["a", 1], ["b", 2], ["c", 3]]
[1, 2, 3]
true
0
2002
[[0, 0], [0, 3], [0, 6], [0, 9], [0, 12], [0, 15]]
