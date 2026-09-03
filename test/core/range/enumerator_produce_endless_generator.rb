# Enumerator.produce(initial) { |prev| ... } yields initial, then each
# block result, forever -- bounded by the consumer. Without initial, the
# first value is block.call(nil).

p(Enumerator.produce(1) { |n| n * 2 }.take(3))
p(Enumerator.produce(1) { |n| n + 1 }.first(4))
g = Enumerator.produce(0) { |n| n + 2 }
p g.next
p g.next
e = Enumerator.produce { |n| (n || 0) + 1 }
p e.take(3)
__END__
[1, 2, 4]
[1, 2, 3, 4]
0
2
[1, 2, 3]
