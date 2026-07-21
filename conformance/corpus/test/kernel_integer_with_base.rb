# Issue #887: Kernel#Integer(s, base) parses non-decimal correctly
# and raises catchable ArgumentError on invalid input.
puts Integer("ff", 16)
puts Integer("1010", 2)
puts Integer("777", 8)
puts Integer("z", 36)
begin
  Integer("zz", 16)
rescue ArgumentError => e
  puts "caught: " + e.message
end
begin
  Integer("xyz", 8)
rescue ArgumentError => e
  puts "caught: " + e.message
end
