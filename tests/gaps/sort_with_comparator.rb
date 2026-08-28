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
