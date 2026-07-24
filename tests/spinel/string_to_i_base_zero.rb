# String#to_i(0) auto-detects the base from the literal prefix.
puts "0xff".to_i(0)
puts "0b10".to_i(0)
puts "0777".to_i(0)
puts "0o755".to_i(0)
puts "42".to_i(0)         # no prefix → decimal
# Sign handling.
puts "-0xff".to_i(0)
puts "+0b101".to_i(0)
