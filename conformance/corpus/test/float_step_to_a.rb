# Float#step without a block materialises into an array.
# Previously SEGV'd because the call fell through to the
# unresolved-call int path (emitting 0), then `.to_a` was
# dispatched on int 0 as if it were a pointer.
puts 1.0.step(3.0, 0.5).to_a.inspect
puts 1.0.step(2.0).to_a.inspect

# Integer#step with int args returns an IntArray.
puts 1.step(5).to_a.inspect
puts 1.step(10, 2).to_a.inspect

# Integer#step with a float step promotes to FloatArray.
puts 1.step(5, 0.5).to_a.inspect

# Empty: limit below start with positive step.
puts 5.0.step(1.0, 1.0).to_a.inspect
puts 5.step(1).to_a.inspect

# Block form is unchanged.
1.0.step(2.0, 0.5) { |x| puts x }
