# Bundled tiny tests (auto-grouped by bundler):
#   - array_bsearch
#   - array_each_index
#   - array_find_index_block
#   - array_group_by_standalone
#   - array_join_poly
#   - array_member_alias
#   - array_product_mixed
#   - array_repeated_combination
#   - int_array_fetch
#   - int_array_one_arg
#   - int_array_reduce_init_sym
#   - poly_array_zip

# === array_bsearch ===
def t_array_bsearch
# Array#bsearch (find-minimum mode) on a sorted typed array: returns the
# first element for which the block is truthy, or nil when none is.
p [1, 3, 5, 7, 9].bsearch { |x| x >= 5 }
p [1, 3, 5, 7, 9].bsearch { |x| x >= 100 }
p [1, 3, 5, 7, 9].bsearch { |x| x >= 1 }
p [2, 4, 6, 8].bsearch { |x| x >= 7 }
p ["a", "b", "c", "d"].bsearch { |s| s >= "c" }
p ["a", "b", "c", "d"].bsearch { |s| s >= "z" }
end
t_array_bsearch

# === array_each_index ===
def t_array_each_index
# Array#each_index { |i| } yields each index (0..length-1). Works on
# any typed array; only the index is bound, no element.
[10, 20, 30].each_index { |i| puts i }
puts "---"
%w[a b c].each_index { |i| puts "idx #{i}" }
puts "---"
[].each_index { |i| puts "never" }
puts "done"
end
t_array_each_index

# === array_find_index_block ===
def t_array_find_index_block
# Issue #864: Enumerable#find_index with block returns the index
# of the first element where the block is truthy, nil otherwise.
# Pre-fix the block form wasn't dispatched (warn + emit 0).
puts [1, 2, 3, 2].find_index { |x| x > 2 }
puts [1, 2, 3].find_index { |x| x > 10 }.inspect
end
t_array_find_index_block

# === array_group_by_standalone ===
def t_array_group_by_standalone
# Array#group_by — standalone (no fused .each) returns a hash
# keyed by the block result with arrays of matching elements.
# Backed by poly_poly_hash + boxed poly_array values; key insertion
# order is preserved.
puts [1,2,3,4,5,6].group_by { |x| x % 3 }.inspect
puts ["apple","ant","bee","cat","cow"].group_by { |s| s[0] }.inspect
end
t_array_group_by_standalone

# === array_join_poly ===
def t_array_join_poly
# Array#join on a mixed-element (poly) array: each element is to_s'd and
# joined with the separator. Homogeneous int_array/str_array have their
# own join; these literals are heterogeneous so they use poly storage.
puts [1, "x", :y].join(",")
puts [1, "x", :y].join
puts [10, "y"].join("-")
puts "done"
end
t_array_join_poly

# === array_member_alias ===
def t_array_member_alias
# Enumerable#member? is the documented alias of #include?. Both
# need dispatching on typed arrays so the alias doesn't fall
# through to the unresolved-call path.
puts [1,2,3].member?(2)
puts [1,2,3].member?(99)
puts ["a","b"].member?("a")
puts ["a","b"].member?("z")
end
t_array_member_alias

# === array_product_mixed ===
def t_array_product_mixed
# Array#product with a single array argument of a different element type
# yields a poly_array of [recv_elem, arg_elem] pairs. Two int arrays keep
# the homogeneous result.
p [1, 2].product(["a", "b"])
p [1, 2, 3].product(["x"])
p ["a", "b"].product([1, 2])
p [1, 2].product([3, 4])
p [1.5, 2.5].product([1, 2])
end
t_array_product_mixed

# === array_repeated_combination ===
def t_array_repeated_combination
# Array#repeated_combination(k) on an int array: k-element combinations
# allowing repeats, materialised as an array of arrays via .to_a.
p [1, 2].repeated_combination(2).to_a
p [1, 2, 3].repeated_combination(2).to_a
p [1, 2].repeated_combination(3).to_a
p [1, 2, 3].repeated_combination(1).to_a
end
t_array_repeated_combination

# === int_array_fetch ===
def t_int_array_fetch
a = [10, 20, 30]
p a.fetch(0)
p a.fetch(2)
p a.fetch(5, 99)
p a.fetch(1, 99)
s = [:x, :y, :z]
p s.fetch(1)
end
t_int_array_fetch

# === int_array_one_arg ===
def t_int_array_one_arg
# Enumerable#one?(x) — true iff exactly one element equals x.
# The block form was already supported; the no-block / arg-form
# fell through to unresolved-call.
puts [1,2,3].one?(2)
puts [1,2,3].one?(99)
puts [1,2,2].one?(2)
puts [].one?(1)
puts [5].one?(5)
end
t_int_array_one_arg

# === int_array_reduce_init_sym ===
def t_int_array_reduce_init_sym
# Enumerable#reduce(init, :op) — explicit init plus a symbol
# operator. Single-arg sym form was already wired; the 2-arg form
# (init + op-sym) fell through to unresolved-call.
puts [1,2,3,4].reduce(:+)
puts [1,2,3,4].reduce(:*)
puts [1,2,3,4].reduce(10, :+)
puts [1,2,3,4].reduce(100, :*)
puts [].reduce(7, :+)
end
t_int_array_reduce_init_sym

# === poly_array_zip ===
def t_poly_array_zip
# Array#zip on heterogeneous (poly_array) receivers — result is a
# poly_array_ptr_array (array of poly_array pairs). The result type
# was being misclassified as int_array_ptr_array, leaving inspect
# walking raw sp_RbVal bytes as ints.
puts [1, "a"].zip([2, "b"]).inspect

# Three heterogeneous arrays
puts [1, "a", :s].zip([2, "b", :t]).inspect
end
t_poly_array_zip

