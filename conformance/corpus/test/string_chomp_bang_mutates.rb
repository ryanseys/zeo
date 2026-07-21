# Issue #859: String#chomp! on a mutable_str mutates in place.
# Pre-fix, chomp! was a no-op (recursed into chomp which produces
# a new string but doesn't write back to the receiver).
# Frozen string literals raise FrozenError per #886.
s = String.new("hello\n")
s.chomp!
puts s.to_s

s2 = String.new("hi\r\n")
s2.chomp!
puts s2.to_s

# upcase! / downcase! / strip! / lstrip! / rstrip! also mutate.
u = String.new("abc")
u.upcase!
puts u.to_s

t = String.new("  spaces  ")
t.strip!
puts t.to_s
