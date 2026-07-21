# Enumerable#chain / Enumerator#+ build an Enumerator::Chain over the
# concatenated sources (#2545, #2548, #2551). The sources are materialized at
# build time, so every terminal the sources support works on the chain.

# Enumerator#+ reports Enumerator::Chain (#2545)
p(([1, 2].each + [3, 4].each).class)
p(([1, 2].each + [3, 4].each).to_a)
p(([1].each + [2].each + [3].each).to_a)

# Struct receiver, chain stored in a variable (#2548)
Nums = Struct.new(:a, :b, :c)
c1 = Nums.new(3, 1, 2).chain([4, 5])
p c1.to_a
p c1.class

# user Enumerable: receiver, argument, stored in a local, block form (#2551)
class E
  include Enumerable
  def initialize(*xs); @xs = xs; end
  def each; @xs.each { |x| yield x }; end
end
a = E.new(1, 2)
c = a.chain([3, 4])
p c.to_a
p [9].chain(E.new(7, 8)).to_a
out = []
E.new(1, 2).chain([3, 4]).each { |x| out << x }
p out

# a user-defined #chain still wins over Enumerable's
class Own
  include Enumerable
  def each; yield 1; end
  def chain(x); "own:#{x}"; end
end
p Own.new.chain(5)

# shapes the eager .to_a path already served
p([1, 2, 3].chain.to_a)
p([1, 2].chain([3], [4, 5]).to_a)
p((1..2).chain([3]).to_a)
p({a: 1}.chain([[:b, 2]]).to_a)

# other terminals over a chain
p([1, 2].chain([3]).map { |x| x * 2 })
p([1, 2].chain([3]).select { |x| x > 1 })
p([1, 2].chain([3]).count)
p([1, 2].chain([3]).sum)
p([1, 2].chain([3]).include?(3))
