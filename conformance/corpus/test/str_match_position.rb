# String#match and String#match? with position argument
# The position is a codepoint index (not byte offset) that limits
# where the regex search starts.

# match? with position
puts "hello".match?(/h/, 1)
puts "hello".match?(/e/, 1)
puts "hello".match?(/o/, -1)
puts "hello".match?(/h/, 0)
puts "hello".match?(/z/, 0)

# match with position
r = "hello".match(/h/, 1); puts r == nil ? "nil" : r[0]
r = "hello".match(/e/, 1); puts r == nil ? "nil" : r[0]
r = "hello".match(/o/, -1); puts r == nil ? "nil" : r[0]
