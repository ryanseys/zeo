# Issue #883: String#to_i with explicit base argument. Without the
# fix, the base was silently dropped and the call returned 0 for
# any non-decimal input.
puts "ff".to_i(16)
puts "1010".to_i(2)
puts "777".to_i(8)
puts "z".to_i(36)
puts "-ff".to_i(16)
puts "1_0".to_i(16)
