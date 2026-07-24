# Issue #921: String#squeeze shrinks the heap-string header
# so length / bytes report the post-squeeze size, not the
# pre-squeeze allocation.
s = "aaabbbccc"
result = s.squeeze
puts result.length
puts result.bytes.inspect
puts result.inspect

# Trailing NULs leak the GC marker byte; ensure none.
puts "xxxxx".squeeze.length
puts "xxxxx".squeeze
