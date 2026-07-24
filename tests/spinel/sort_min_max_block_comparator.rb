# Array#sort / min / max with a 2-param comparator block.
# Block returns -1/0/1; sort uses it for ordering, min/max pick
# the element that the comparator places at the extremum.

# Sort with reverse comparator
puts [3,1,4,1,5].sort { |a,b| b <=> a }.inspect
puts ["c","a","b"].sort { |a,b| b <=> a }.inspect

# Sort with custom comparator
puts ["banana","cat","ape"].sort { |a,b| a.length <=> b.length }.inspect

# min/max with reverse comparator inverts the result
puts [3,1,4,1,5].min { |a,b| b <=> a }
puts [3,1,4,1,5].max { |a,b| b <=> a }

# min/max with length comparator
puts ["banana","cat","ape"].min { |a,b| a.length <=> b.length }
puts ["banana","cat","ape"].max { |a,b| a.length <=> b.length }
