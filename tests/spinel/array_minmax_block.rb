# Array#minmax with a 2-param comparator block honours the block's
# ordering instead of the default < / >.
p [3, 1, 2].minmax { |a, b| b <=> a }   # reversed: [3, 1]
p [3, 1, 2].minmax { |a, b| a <=> b }   # normal:   [1, 3]
p [5, 2, 8, 1, 9].minmax { |a, b| a <=> b }
p [5, 2, 8, 1, 9].minmax { |a, b| b <=> a }

# String elements, comparing by length.
p ["bb", "a", "ccc"].minmax { |a, b| a.length <=> b.length }
p ["bb", "a", "ccc"].minmax { |a, b| b.length <=> a.length }

# Single-element and the no-block form still work.
p [42].minmax { |a, b| a <=> b }
p [3, 1, 2].minmax
