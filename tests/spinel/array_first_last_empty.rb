# Array#first / Array#last on an empty array return nil,
# not SP_INT_NIL printed as a huge negative number.
# CRuby returns nil for [].first and [].last.

puts [].first.inspect
puts [].last.inspect
puts [].first.nil?.inspect
puts [].last.nil?.inspect
puts [1, 2, 3].first.inspect
puts [1, 2, 3].last.inspect
puts [1, 2, 3].first(2).inspect
puts [1, 2, 3].last(2).inspect
