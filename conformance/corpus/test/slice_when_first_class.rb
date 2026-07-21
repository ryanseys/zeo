# int_array.slice_when { |a,b| cond }.to_a returns a first-class array of slices
# (a new slice starts before b whenever the block is truthy).
p [1, 2, 4, 5, 7].slice_when { |a, b| b - a > 1 }.to_a
p [1, 2, 3].slice_when { |a, b| false }.to_a
p [1, 2, 3].slice_when { |a, b| true }.to_a
p [42].slice_when { |a, b| a < b }.to_a
p [1, 3, 5, 2, 4].slice_when { |a, b| a > b }.to_a
# the result is a real array: chain further
p [1, 2, 4, 5].slice_when { |a, b| b - a > 1 }.to_a.map { |s| s.sum }
p [1, 2, 4, 5, 7].slice_when { |a, b| b - a > 1 }.to_a.length
# chunk_while (the inverse) still works
p [1, 2, 4, 9, 10, 11, 12, 0].chunk_while { |a, b| b - a == 1 }.to_a
