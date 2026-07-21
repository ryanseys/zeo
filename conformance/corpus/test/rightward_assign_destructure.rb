# Rightward assignment with an array / hash pattern (MatchRequiredNode):
# `value => [a, b, c]` destructures and binds, raising
# NoMatchingPatternError when the value does not match.

# Integer array.
[1, 2, 3] => a, b, c
puts a
puts b
puts c

# String array (binds string-typed locals).
["x", "y"] => p, q
puts p
puts q

# Hash pattern binds by key.
{name: "Alice", age: 30} => {name:, age:}
puts name
puts age

# Length mismatch raises NoMatchingPatternError.
begin
  [1, 2] => d, e, f
  puts "no raise"
rescue NoMatchingPatternError
  puts "raised"
end
