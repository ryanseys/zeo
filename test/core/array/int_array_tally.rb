# Array#tally maps each distinct element to the number of times it occurs.
# The result is an ordinary Hash: a key it does not hold answers nil, and
# nil is not 0.
result = [1, 2, 2, 3, 3, 3].tally
puts result[1]
puts result[2]
puts result[3]
puts result.length
puts result.inspect
puts result.has_key?(2)
puts result.has_key?(99)
# A missing key is nil, so this is false rather than true.
puts result[99] == 0
__END__
1
2
3
3
{1 => 1, 2 => 2, 3 => 3}
true
false
false
