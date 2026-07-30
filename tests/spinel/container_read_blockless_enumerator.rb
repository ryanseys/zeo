# A blockless enumerator on a value reached through a container -- a String
# destructured out of an Array, an Array held as a Hash value -- materializes
# like the statically typed receiver's does, instead of staying untyped.
pair = [0, "hello"]
i, text = pair
p text.each_byte.reduce(0) { |a, b| a + b }
p text.each_char.to_a
p text.each_byte.to_a.size
p text.each_char.map { |ch| ch.upcase }.join
p text.each_line.to_a
p text.each_codepoint.sum
p text.upcase
p text.length

[[1, "ab"]].each do |n, s|
  p s.each_char.to_a
  p s.each_byte.sum
end

g = { "k" => [1, 2, 3, 4] }
p g["k"].each_cons(2).map { |a, b| b - a }
p g["k"].combination(2).select { |a, b| a < b }.size
p g["k"].each_slice(2).to_a
p g["k"].permutation(2).to_a.size
