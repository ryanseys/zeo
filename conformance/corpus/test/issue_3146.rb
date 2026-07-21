# Symbol-keyed Hash literal + grow past the initial capacity. The grow path's
# capacity->size_t allocation must not trip -Walloc-size-larger-than (#3146),
# and must rehash correctly.
h = {x: 1}
p h[:x]

big = {}
30.times { |i| big[:"key#{i}"] = i * 3 }
p big[:key0]
p big[:key29]
p big.length
p big[:missing]
