# `Range#step` over a NON-numeric range walks by `succ`, in both the block
# and the blockless form.
#
# `docs/COMPATIBILITY.md` recorded this as the one outright panic in the
# document -- "answers the right Enumerator, but WALKING it panics". It
# does not: every row below matches ruby 4.0.6, so the note is deleted and
# these rows replace it.

p ("a".."e").step(2).to_a
out = []
("a".."e").step(2) { |v| out << v }
p out
p ("a".."e").step(1).to_a
p ("aa".."ad").step(2).to_a
p ("a".."e").step(2).class
p (:a..:e).step(2).to_a

# A user class with `succ` and `<=>` walks the same way. Ruby reaches for
# `+` first and only falls back to `succ`, so a class without `+` raises
# -- which is the row that pins WHICH protocol is being used.
class Countable
  include Comparable
  attr_reader :n

  def initialize(n) = @n = n
  def succ = Countable.new(@n + 1)
  def <=>(o) = @n <=> o.n
  def inspect = "C#{@n}"
end
begin
  (Countable.new(1)..Countable.new(5)).step(2).to_a
rescue NoMethodError => e
  puts e.message
end

# Every numeric range keeps its ArithmeticSequence.
p (1..9).step(2).class
p (1..9).step(2).to_a
__END__
["a", "c", "e"]
["a", "c", "e"]
["a", "b", "c", "d", "e"]
["aa", "ac"]
Enumerator
[:a, :c, :e]
undefined method '+' for an instance of Countable
Enumerator::ArithmeticSequence
[1, 3, 5, 7, 9]
