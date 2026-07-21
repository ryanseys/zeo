# Array#slice_before / #slice_after on int_array with a literal
# int delimiter.
puts [1, 2, 3, 4, 5].slice_before(3).to_a.inspect
puts [1, 2, 3, 4, 5].slice_after(3).to_a.inspect
# Empty matches: delimiter never appears.
puts [1, 2, 3].slice_before(99).to_a.inspect
