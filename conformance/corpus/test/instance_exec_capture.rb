# Outer-local capture in a direct instance_exec block. Call-site
# inline-splicing runs the block body in the caller's scope, so the
# block both reads the caller's locals and writes them back -- the
# mutation persists after the call, matching CRuby's closure semantics.
#
# Capture only applies to heap receivers (the inlined path); a subclass
# keeps Box heap-allocated.

class Box
  def initialize(v)
    @v = v
  end
end

class BoxPlus < Box
end

b = Box.new(10)

# Read an outer local, write it back: the mutation is visible afterward.
acc = 100
b.instance_exec { acc = acc + @v }
puts acc                         #=> 110

# Accumulate across successive calls against the same captured local.
b.instance_exec { acc = acc + @v }
b.instance_exec { acc = acc + @v }
puts acc                         #=> 130

# Capture alongside a block param.
total = 0
b.instance_exec(5) { |x| total = total + @v + x }
puts total                       #=> 15

# Capture in expression position: the block reads outer locals and its
# value flows out as the call's value.
base = 7
r = b.instance_exec(3) { |x| base + @v + x }
puts r                           #=> 20

# Write-back in expression position: mutate a captured local and also
# use the call's value.
n = 1
m = b.instance_exec { n = n + @v; n }
puts n                           #=> 11
puts m                           #=> 11

puts "done"
