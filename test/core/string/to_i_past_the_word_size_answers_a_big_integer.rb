# String#to_i and Integer() on input too large for a machine word answer a
# big integer rather than raising or saturating. Base 16 goes the same way,
# and an in-range value is unaffected.

begin
  "99999999999999999999".to_i
  puts "no exception"
rescue RangeError
  puts "to_i raised RangeError"
end

begin
  "ffffffffffffffff0".to_i(16)
  puts "no exception"
rescue RangeError
  puts "to_i(16) raised RangeError"
end

begin
  Integer("99999999999999999999")
  puts "no exception"
rescue RangeError
  puts "Integer raised RangeError"
end

# In-range values still work.
puts "42".to_i
puts "ff".to_i(16)
puts Integer("-1000")
__END__
no exception
no exception
no exception
42
255
-1000
