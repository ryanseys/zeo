# Cases where a compiler is tempted to reject at build time but real Ruby
# raises at RUNTIME -- so the program must compile, run, and let a `rescue`
# see the error. Codegen lowers every branch eagerly, so rejecting any of
# these statically would kill an otherwise-correct program.

def err
  yield
rescue => e
  "#{e.class}: #{e.message}"
end

# 1. public_send is stricter than an explicit-receiver call: it rejects BOTH
#    private and protected, with no self-relatedness relaxation.
class Acct
  def balance = 100
  def peer_check(other) = other.guarded

  protected

  def guarded = "prot"

  private

  def secret = 42
end

a = Acct.new
p a.public_send(:balance)
puts err { a.public_send(:secret) }
puts err { a.public_send(:guarded) }

# send / __send__ stay visibility-blind
p a.send(:secret)
p a.__send__(:secret)

# ...while a protected method IS callable when the caller's self is related.
p a.peer_check(Acct.new)

# A runtime-computed method name is checked per resolved arm.
def dyn(o, m) = o.public_send(m)
p dyn(a, :balance)
puts err { dyn(a, :secret) }

# 2. A blockless Thread.new / Fiber.new raises rather than failing to compile.
puts err { Thread.new }
puts err { Fiber.new }

# 3. A non-finite float literal is a perfectly good value, not a codegen
#    failure -- it just cannot be spelled as a Rust float token.
big = 1e400
p big
p(-1e400)
p big.infinite?
p (big - big).nan?

# 4. `for` performs no type dispatch in Ruby: it is just `each` with a block,
#    so any object answering #each iterates.
class Nums
  include Enumerable
  def initialize(*xs) = @xs = xs
  def each; @xs.each { |x| yield x }; end
end

total = 0
for x in Nums.new(1, 2, 3, 4)
  total += x
end
p total

# multi-assignment `for` over a pair-yielding each
class Pairs
  include Enumerable
  def each; yield [1, :a]; yield [2, :b]; end
end

pairs = []
for k, v in Pairs.new
  pairs << "#{k}:#{v}"
end
p pairs

# `for` does NOT introduce a new scope: the variable survives the loop.
for survivor in [10, 20, 30]
end
p survivor

# an object with no #each fails at runtime, not at compile time
puts err { for z in 5; end }

# 5. Guards against loops that would otherwise never terminate.
#
# NOTE: `"x" * (1 << 60)` is deliberately NOT exercised here. CRuby's guard
# only covers the length multiplication, so a 1-byte string times 2^60 reaches
# the allocator and raises NoMemoryError -- which, not being a StandardError,
# escapes `rescue => e` and kills the program. zeo caps the total size and
# raises a catchable ArgumentError instead, so the two cannot share a golden
# file; the corpus `string_multiply_overflow` pins that behavior separately.
puts err { "x" * -1 }
p "ab" * 3
puts err { 1.step(10, 0) { } }
puts err { Rational(1, 2).step(Rational(5, 2), 0) { } }
puts err { (1..).to_a }
p (1..4).to_a
__END__
100
NoMethodError: private method 'secret' called for an instance of Acct
NoMethodError: protected method 'guarded' called for an instance of Acct
42
42
"prot"
100
NoMethodError: private method 'secret' called for an instance of Acct
ThreadError: must be called with a block
ArgumentError: tried to create Proc object without a block
Infinity
-Infinity
1
true
10
["1:a", "2:b"]
30
NoMethodError: undefined method 'each' for an instance of Integer
ArgumentError: negative argument
"ababab"
ArgumentError: step can't be 0
ArgumentError: step can't be 0
RangeError: cannot convert endless range to an array
[1, 2, 3, 4]
