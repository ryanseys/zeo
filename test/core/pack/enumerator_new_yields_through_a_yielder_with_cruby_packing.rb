# `Enumerator.new { |y| ... }` -- the generator/Yielder pair, and the
# packing truth table: `next_values` preserves yield arity exactly,
# `next` collapses through ary2sv (`yield`->nil, `yield nil`->nil,
# `yield 1,2`->[1,2], `yield [3,4]`->[3,4]); the block's return value is
# the StopIteration result; `rewind` restarts from the top.

g = Enumerator.new do |y|
  y.yield
  y.yield nil
  y.yield 1, 2
  y << [3, 4]
  :fin
end
p g.next_values
p g.next_values
p g.next_values
p g.next_values
g.rewind
p g.next
p g.next
p g.next
p g.next
begin
  g.next
rescue StopIteration => e
  p e.result
end
__END__
[]
[nil]
[1, 2]
[[3, 4]]
nil
nil
[1, 2]
[3, 4]
:fin
