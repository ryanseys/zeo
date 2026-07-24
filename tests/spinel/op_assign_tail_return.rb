# An operator-assignment (x += 1, @x += 1) in method tail position returns the
# updated value, like the class-variable form already did. (matz/spinel#1484)
class CounterLocal
  def inc(v); v += 1; end
  def dec(v); v -= 2; end
  def scale(v); v *= 3; end
end
class CounterIvar
  attr_accessor :value
  def initialize(v); @value = v; end
  def inc; @value += 1; end
  def add(n); @value += n; end
end
class CounterCvar
  @@v = 10
  def self.inc; @@v += 1; end
end

c = CounterLocal.new
puts c.inc(10)     # 11
puts c.dec(10)     # 8
puts c.scale(4)    # 12

iv = CounterIvar.new(10)
puts iv.inc        # 11
puts iv.inc        # 12
puts iv.add(5)     # 17
puts iv.value      # 17

puts CounterCvar.inc   # 11
puts CounterCvar.inc   # 12

# op-assign tail inside a conditional branch still returns the value
def branchy(x)
  if x > 0
    x += 100
  else
    x -= 100
  end
end
puts branchy(5)    # 105
puts branchy(-5)   # -105
