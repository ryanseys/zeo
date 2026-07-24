# Array#tally on int_array maps each distinct element to its
# occurrence count. Result is an int_int_hash (new variant
# alongside sym_int_hash / str_int_hash etc.).
result = [1, 2, 2, 3, 3, 3].tally
puts result[1]
puts result[2]
puts result[3]
puts result.length
puts result.inspect
puts result.has_key?(2)
puts result.has_key?(99)
# missing-key returns 0 in spinel today (vs nil in CRuby) —
# the broader nullable-hash semantics are tracked in #801.
puts result[99] == 0
