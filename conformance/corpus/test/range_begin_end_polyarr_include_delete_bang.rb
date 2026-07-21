# Three small adjacent fixes:
#   - Range#begin / Range#end (aliases of first / last)
#   - Array#include? on poly_array
#   - String#delete_prefix! / delete_suffix! on mutable_str

puts (1..10).begin
puts (1..10).end

a = [1, "hello", 3.0]
puts a.include?(1)
puts a.include?("hello")
puts a.include?(3.0)
puts a.include?(99)

s = String.new("hello world")
s.delete_prefix!("hello ")
puts s.to_s

s2 = String.new("hello world")
s2.delete_suffix!(" world")
puts s2.to_s
