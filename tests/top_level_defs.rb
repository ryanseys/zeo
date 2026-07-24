# A top-level `def` is a PRIVATE instance method of Object -- so it is
# callable by implicit self from anywhere (top level, any method body, any
# class body), invisible to `respond_to?`, and reachable through `send`.

def greet(name)
  "Hello, #{name}!"
end

puts greet("Ada")

# Callable from inside a class's instance method...
class Speaker
  def speak
    greet("from-instance")
  end
end
puts Speaker.new.speak

# ...from a class METHOD...
class Factory
  def self.build
    greet("from-class-method")
  end
end
puts Factory.build

# ...from a class BODY...
class AtBody
  RESULT = greet("from-class-body")
end
puts AtBody::RESULT

# ...and from another top-level def.
def outer
  greet("from-another-def")
end
puts outer

# It is PRIVATE on Object: invisible to respond_to?, reachable via send.
p self.respond_to?(:greet)
p self.respond_to?(:greet, true)
p send(:greet, "via-send")
p Speaker.new.respond_to?(:greet)
p Speaker.new.send(:greet, "on-an-instance")

# Calling it with an explicit receiver is a NoMethodError (it's private).
begin
  Speaker.new.greet("explicit")
rescue NoMethodError => e
  puts "NoMethodError caught"
end

# Full parameter support: optional, rest, post, keywords, block.
def every_kind(a, b = 2, *rest, c, d:, e: 5, **kw, &blk)
  parts = [a, b, rest, c, d, e, kw]
  parts << blk.call if blk
  parts
end
p every_kind(1, :post, d: 4)
p every_kind(1, 9, :r1, :r2, :post, d: 4, e: 50, x: 1)
p(every_kind(1, :post, d: 4) { "block!" })

# Endless top-level defs.
def square(x) = x * x
p square(7)

def zero = 0
p zero

# A top-level def can yield.
def each_pair
  yield 1, 2
  yield 3, 4
end
each_pair { |a, b| p [a, b] }

# Recursion works.
def fact(n)
  n <= 1 ? 1 : n * fact(n - 1)
end
p fact(6)

# Mutual recursion between two top-level defs.
def even?(n) = n.zero? ? true : odd?(n - 1)
def odd?(n) = n.zero? ? false : even?(n - 1)
p [even?(10), odd?(7)]

# A later def REPLACES an earlier one of the same name.
def replaced = "first"
def replaced = "second"
p replaced

# `__method__` reports the running method's name.
def whoami = __method__
p whoami

# A top-level def is reachable as a Method object.
m = method(:square)
p m.call(9)
p m.name

# And usable as a block via &.
p [1, 2, 3].map(&method(:square))

# Top-level defs see top-level constants.
LIMIT = 10
def under_limit?(n) = n < LIMIT
p under_limit?(3)
p under_limit?(30)
