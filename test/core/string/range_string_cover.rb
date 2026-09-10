# Range#include?, cover? and === where the endpoints are Strings rather than
# numbers: the comparison is String ordering.

puts ("a".."z").include?("m")
puts ("a".."z").cover?("m")
puts ("a".."z").include?("Z")
puts ("a".."z").cover?("a")
puts ("a".."z").cover?("z")
puts ("a"..."z").cover?("z")  # exclusive end
__END__
true
true
false
true
true
false
