# Issue #911: Array#include? on a typed array with a wrong-type
# argument returns false instead of failing to C-compile.
puts [1, 2, 3].include?("hello")
puts [1, 2, 3].include?(2)
puts ["a", "b", "c"].include?(42)
puts ["a", "b", "c"].include?("b")
