# Consolidated CRuby-4.0.5 parity anchor for direct instance_exec: one
# file exercising the supported core shapes in combination. Every line's
# output is cross-checked against `ruby` (the .expected is the oracle).

class Acc
  def initialize(base)
    @base = base
  end

  def base
    @base
  end

  def combine(x, y)
    @base + x + y
  end
end

class AccPlus < Acc
end

a = AccPlus.new(100)

# positional args bound to block params; body calls a rebound-self method
puts a.instance_exec(3, 4) { |x, y| combine(x, y) }      #=> 107

# expression position: call value is the block's last expression
puts a.instance_exec(5) { |x| base + x }                 #=> 105

# outer-local capture: read + write-back into the caller's scope
total = 0
a.instance_exec { total = base + 1 }
puts total                                               #=> 101

# capture accumulation across multiple calls
sum = 0
a.instance_exec { sum = sum + base }
a.instance_exec { sum = sum + base }
puts sum                                                 #=> 200

# block body-local + multi-statement
puts a.instance_exec(2) { |x| t = base + x; t * 2 }      #=> 204

puts "done"
