# A container operation that finds nothing answers nil, and `== nil` on it is
# therefore true. Each line below reads past the end, off an empty container,
# or for a key or element that is not there:
#
#   arr[i], arr.delete_at(i)     -- index outside the array
#   arr.first, arr.last, arr.pop -- the array is empty
#   arr.find_index(x)            -- x is not in the array
#   hash[k]                      -- the hash has no such key
#   regex.match(s)               -- the pattern does not match
#
# The hazard is a container whose element type cannot hold nil: a miss must
# still answer nil rather than that type's zero value.

p ([1, 2, 3][99] == nil)          # 01
p Array.new.instance_of?(Array)   # 02
p ([1, 2].delete_at(3) == nil)    # 03
p ([].first == nil)               # 04
p ([].last == nil)                # 05
p ([].pop == nil)                 # 06
p ([1, 2].find_index(3) == nil)   # 07
p ({0 => 0}[5] == nil)            # 08
p (/xyz/.match("abxyc") == nil)   # 09
__END__
true
true
true
true
true
true
true
true
true
