# A combination / each_cons chain whose receiver is an Array bound by
# destructuring a Hash iteration's block parameter compiles and folds like
# the same chain on a plain local Array.
h = { "a" => [1, 2, 3] }
p h.flat_map { |k, rows| rows.combination(2).select { |a, b| a < b }.map { |a, b| a + b } }
p h.flat_map { |k, rows| rows.combination(2).to_a.size }
p h.map { |k, rows| rows.each_cons(2).map { |a, b| a + b } }
