# Test control flow additions

# __LINE__
puts __LINE__  # 4

# defined? on known variable
x = 42
puts defined?(x) ? "yes" : "no"  # yes

# String freeze (no-op in AOT)
s = "hello"
puts s  # hello

# Modifier if/unless
y = 10
puts "big" if y > 5     # big
puts "small" unless y > 100  # small

# Ternary chains
a = 3
puts(a == 1 ? "one" : a == 2 ? "two" : a == 3 ? "three" : "other")  # three

puts "done"
