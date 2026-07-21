# Range#include? / cover? / === on string ranges. Spinel inlines
# strcmp bounds for literal string-range receivers rather than
# trying to fit the receiver into the int-only sp_Range struct.

puts ("a".."z").include?("m")
puts ("a".."z").cover?("m")
puts ("a".."z").include?("Z")
puts ("a".."z").cover?("a")
puts ("a".."z").cover?("z")
puts ("a"..."z").cover?("z")  # exclusive end
