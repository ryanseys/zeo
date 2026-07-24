# Issue #836: String * <huge> raises ArgumentError instead of
# segfaulting (the implicit malloc-NULL + memcpy chain).
begin
  puts "x" * (1 << 60)
rescue ArgumentError => e
  puts "huge: " + e.message
end
begin
  puts "x" * -1
rescue ArgumentError => e
  puts "neg: " + e.message
end
puts "ab" * 3
puts ("hi" * 0).inspect
