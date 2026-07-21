# Array#slice_when and Array#chunk on int_array. Materialises
# directly (no Enumerator); .to_a is a no-op on the result.

puts [1, 2, 3, 4, 5].slice_when { |a, b| b > 3 }.to_a.inspect
puts [1, 2, 2, 3, 3, 3, 4].chunk { |x| x }.to_a.inspect
# Empty array: empty result.
puts [].slice_when { |a, b| true }.to_a.inspect
