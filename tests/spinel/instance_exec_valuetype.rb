# A value-type receiver (an immutable, scalar-only class the compiler
# promotes to a by-value struct) works with direct instance_exec via
# inline-splice. self is rebound to a by-value copy of the receiver, so
# ivar reads, bare rebound method calls, block params, expression-
# position values and outer-local capture all resolve against it. (A
# value type is immutable by construction, so there is no self-ivar
# write-back to observe -- matching how the receiver is passed by value.)

class Vec
  def initialize(x, y)
    @x = x
    @y = y
  end

  def x
    @x
  end

  def sum
    @x + @y
  end
end

v = Vec.new(3, 4)

# ivar read in the block.
puts(v.instance_exec { @x })               #=> 3

# operator over two ivar reads.
puts(v.instance_exec { @x * @y })          #=> 12

# bare rebound method call against the value-type self.
puts(v.instance_exec { sum })              #=> 7

# positional arg bound to a block param, combined with a rebound call.
puts(v.instance_exec(10) { |k| sum + k })  #=> 17

# expression position: the call value flows into an outer local.
r = v.instance_exec { @x + 100 }
puts r                                     #=> 103

# outer-local capture: written from inside the block (caller scope, so
# it propagates regardless of by-value self).
acc = 0
v.instance_exec { acc = sum + 1 }
puts acc                                   #=> 8

puts "done"
