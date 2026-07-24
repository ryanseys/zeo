# String#concat accepts any number of arguments (appended left to right);
# `<<` is the single-argument operator spelling.
s = "a"
s.concat("b", "c", "d")
puts s

t = "greet"
t << "-" << "world"
puts t

# Integer arguments append the corresponding codepoint's character.
u = "n"
u.concat(0x61, 0x62)
puts u

# No arguments is a no-op that returns the receiver.
puts "unchanged".concat

# Mixed string and codepoint arguments.
v = "x"
v.concat("y", 0x7A, "!")
puts v
