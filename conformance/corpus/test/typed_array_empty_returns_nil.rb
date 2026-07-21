# Issue #832: int_array pop/shift/min/max on empty return nil
# (int? sentinel SP_INT_NIL) per MRI. Non-empty returns the value.
# float_array is deferred (needs float? sentinel design).
a = [1, 2]
2.times { a.pop }
puts "empty int pop: " + a.pop.inspect

b = [1, 2]
2.times { b.shift }
puts "empty int shift: " + b.shift.inspect

puts "empty int min: " + [].min.inspect
puts "empty int max: " + [].max.inspect

# Non-empty still works.
puts [1, 2, 3].pop
puts [1, 2, 3].shift
puts [3, 1, 2].min
puts [3, 1, 2].max
