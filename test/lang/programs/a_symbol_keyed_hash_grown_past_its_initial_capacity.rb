# Every key is still findable after the rehash.
# (spinel issue #3146)
h = {x: 1}
p h[:x]

big = {}
30.times { |i| big[:"key#{i}"] = i * 3 }
p big[:key0]
p big[:key29]
p big.length
p big[:missing]
__END__
1
0
87
30
nil
