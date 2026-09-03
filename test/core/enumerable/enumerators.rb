# Phase 17.2 -- the fiber-backed Enumerator (per CRuby's enumerator.c):
# blockless iteration methods return real Enumerators, external iteration
# (#next/#peek) suspends a coroutine per element, StopIteration carries the
# iteration's result, and Kernel#loop swallows it.

e = [10, 20, 30].each
p e
r = loop do
  puts e.next
end
p r

e2 = [1, 2].each
p e2.peek
p e2.peek
p e2.next
p e2.next
begin
  e2.next
rescue StopIteration => ex
  puts "done: #{ex.message}"
  p ex.result
end
e2.rewind
p e2.next

# Blockless breadth, chained through Enumerable.
p [1, 2, 3].map
p [1, 2, 3].map.each { |x| x * 3 }
p 5.times.to_a
p 2.upto(5).to_a
p (1..10).step(3).to_a
p "hey".each_char.to_a
p({ a: 1, b: 2 }.each_value.to_a)
p %w[a b c].each_with_index.to_a
p [10, 20].map.with_index { |x, i| x * i }
p [10, 20].each.with_index(5).to_a
p [4, 2, 6].each.sort
p [1, 2].each.with_object([]) { |x, memo| memo << x * 2 }

# Enumerator.new: the generator/Yielder pair, lazy even when infinite.
squares = Enumerator.new do |y|
  n = 1
  loop do
    y << n * n
    n += 1
  end
end
p squares.next
p squares.next
p squares.take(5)
p squares.first(3)

pairs = Enumerator.new { |y| y.yield 1, 2 }
p pairs.next_values
pairs.rewind
p pairs.next

# size never iterates.
p [1, 2, 3].each.size
p 5.times.size
p [1, 2, 3].each_slice(2).size
p Enumerator.new(4) { |y| y << 1 }.size

# to_enum on a user class -- the real-Ruby blockless-each pattern.
class Deck
  include Enumerable
  def initialize(cards)
    @cards = cards
  end
  def each
    return to_enum(:each) unless block_given?
    i = 0
    while i < @cards.length
      yield @cards[i]
      i += 1
    end
    self
  end
end
d = Deck.new([5, 3, 9])
p d.each.next
p d.each.sort
p d.map.with_index { |c, i| "#{i}:#{c}" }

# Struct's synthesized each uses the same pattern.
Point = Struct.new(:x, :y)
en = Point.new(1, 2).each
p en.class
p en.to_a

p(loop { break 42 })
p(loop { raise StopIteration })
__END__
#<Enumerator: [10, 20, 30]:each>
10
20
30
[10, 20, 30]
1
1
1
2
done: iteration reached an end
[1, 2]
1
#<Enumerator: [1, 2, 3]:map>
[3, 6, 9]
[0, 1, 2, 3, 4]
[2, 3, 4, 5]
[1, 4, 7, 10]
["h", "e", "y"]
[1, 2]
[["a", 0], ["b", 1], ["c", 2]]
[0, 20]
[[10, 5], [20, 6]]
[2, 4, 6]
[2, 4]
1
4
[1, 4, 9, 16, 25]
[1, 4, 9]
[1, 2]
[1, 2]
3
5
2
4
5
[3, 5, 9]
["0:5", "1:3", "2:9"]
Enumerator
[1, 2]
42
nil
