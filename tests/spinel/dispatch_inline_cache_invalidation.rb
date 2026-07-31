# A dynamic call site remembers what the receiver's class resolved to, so
# anything that can change that answer has to be seen. Every case below WARMS
# the site first -- a cache that was never filled would pass these by accident.

class Greeter
  def hello = "plain"
end

def call_hello(g) = g.hello

warm = Greeter.new
100.times { call_hello(warm) }
p call_hello(warm)

# A runtime `define_method` replaces it, on the warmed receiver and a fresh one.
Greeter.define_method(:hello) { "redefined" }
p call_hello(warm)
p call_hello(Greeter.new)

# A per-object singleton is closer than the class.
solo = Greeter.new
100.times { call_hello(solo) }
def solo.hello = "singleton"
p call_hello(solo)
p call_hello(Greeter.new)

# `extend` splices a module ahead of nothing, but `prepend` beats the class.
module Loud
  def hello = "LOUD"
end
ext = Greeter.new
100.times { call_hello(ext) }
ext.extend(Loud)
p call_hello(ext)

class Prepended
  def hello = "base"
end
pre = Prepended.new
100.times { call_hello(pre) }
Prepended.prepend(Loud)
p call_hello(pre)
p call_hello(Prepended.new)

# A second class at the same site: the cache holds one, the other must still
# resolve correctly.
class Other
  def hello = "other"
end
mixed = [Greeter.new, Other.new, Greeter.new, Other.new]
p mixed.map { |m| call_hello(m) }

# The same site over builtin VALUE receivers, which resolve by a different
# route but share the cache.
def stringify(v) = v.to_s
100.times { stringify(1) }
p [stringify(1), stringify(2.5), stringify(:sym), stringify([1, 2]), stringify(nil)]

# `undef` retracts a name after the site is warm.
class Fragile
  def hello = "here"
end
f = Fragile.new
100.times { call_hello(f) }
class Fragile
  undef_method :hello
end
begin
  call_hello(f)
rescue NoMethodError
  p :undefined
end

# An alias added at runtime resolves through the same site.
class Greeter
  def real = "real"
end
g2 = Greeter.new
100.times { call_hello(g2) }
Greeter.alias_method :hello, :real
p call_hello(g2)
