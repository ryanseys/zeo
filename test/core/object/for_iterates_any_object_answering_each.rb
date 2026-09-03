# Ruby's `for` performs NO type dispatch: `for x in obj` compiles to
# `obj.each { |x| ... }` (compile_iter, compile.c:8548). So a user class
# iterates, a pair-yielding each destructures, the loop variable outlives
# the loop (`for` introduces no scope), and an object with no `each`
# fails at RUNTIME rather than failing the compile.

class Nums
  include Enumerable
  def initialize(*xs) = @xs = xs
  def each; @xs.each { |x| yield x }; end
end
class Pairs
  include Enumerable
  def each; yield [1, :a]; yield [2, :b]; end
end
total = 0
for x in Nums.new(1, 2, 3, 4)
  total += x
end
p total
pairs = []
for k, v in Pairs.new
  pairs << "#{k}:#{v}"
end
p pairs
for survivor in [10, 20, 30]
end
p survivor
begin
  for z in 5; end
rescue NoMethodError => e
  puts e.message
end
__END__
10
["1:a", "2:b"]
30
undefined method 'each' for an instance of Integer
