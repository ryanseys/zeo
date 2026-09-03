# `to_enum`/`enum_for` on a user class -- the real-Ruby
# `return to_enum(:each) unless block_given?` pattern -- and the
# synthesized Struct `each` using exactly that pattern.

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
e = d.each
p e.class
p e.next
p d.each.sort
p d.map.with_index { |c, i| "#{i}:#{c}" }
p 7.to_enum(:upto, 9).to_a
P17 = Struct.new(:x, :y)
pt = P17.new(1, 2)
en = pt.each
p en.class
p en.next
p en.to_a
__END__
Enumerator
5
[3, 5, 9]
["0:5", "1:3", "2:9"]
[7, 8, 9]
Enumerator
1
[1, 2]
