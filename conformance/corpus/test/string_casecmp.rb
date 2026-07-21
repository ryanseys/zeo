# String#casecmp returns -1 / 0 / 1 for case-insensitive lex compare.
# String#casecmp? returns true / false.
puts "ABC".casecmp("abc")
puts "ABC".casecmp("abd")
puts "abc".casecmp("ABB")
puts "ABC".casecmp?("abc")
puts "ABC".casecmp?("abd")
puts "".casecmp("")
puts "a".casecmp("aa")
