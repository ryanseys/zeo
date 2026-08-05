# `Integer(str, 0)` means "detect the base from the literal prefix" (0x, 0b,
# 0o/0, else decimal) -- the C strtol convention. zeo's Integer() only accepts
# explicit bases and raises ArgumentError for base 0.
puts Integer("0b101", 0)
puts Integer("0x1A", 0)
puts Integer("0o17", 0)
puts Integer("017", 0)
puts Integer("42", 0)
begin
  Integer("9", 8)
rescue ArgumentError => e
  puts "control: #{e.message}"
end
