# Regexp.last_match(n) returns the n-th capture group of the most
# recent match (n==0 is the whole match), or nil when the group is
# out of range / didn't participate. Before, the call resolved to the
# NoMethodError gate and emitted 0.

"johndoe@example.com".match(/(\w+)@(\w+)/)
puts Regexp.last_match(0)   # whole match
puts Regexp.last_match(1)   # first group
puts Regexp.last_match(2)   # second group

# Variable index
i = 1
puts Regexp.last_match(i)

# Out-of-range group -> nil
p Regexp.last_match(7)

# A group that didn't participate -> nil
"abc".match(/(x)|(abc)/)
p Regexp.last_match(1)
puts Regexp.last_match(2)
