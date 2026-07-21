# Bundled tests:
#   - array_minmax
#   - array_dig
#   - array_difference
#   - array_intersection
#   - array_union

# === array_minmax ===
def t_array_minmax
  p [3,1,4,1,5,9,2,6].minmax
  p [3,1,2].minmax
  # Float-array minmax returns tuple:float,float — exercises the
  # matching inspect arm.
  p [1.5, 0.5, 2.5].minmax
end
t_array_minmax

# === array_dig ===
def t_array_dig
  a = [10, 20, 30]
  # In-bounds — first / middle / last.
  p a.dig(0)
  p a.dig(1)
  p a.dig(2)
  # Negative indices — counts from the end.
  p a.dig(-1)
  p a.dig(-3)
  # Out-of-bounds dig isn't covered here. Ruby returns nil; Spinel's
  # Array#[] inherits its existing in-bounds-only contract on int_array
  # (the single-arg dig delegates to []), so out-of-bounds reads behave
  # like out-of-bounds [] reads. Adding nil-returning bounds checks is
  # a separate scope (would widen the result type to poly).
end
t_array_dig

# === array_difference ===
def t_array_difference
  # Array#difference for typed arrays (int/sym/str/float).
  # Mirrors Array#intersection (c31b618) and Array#union — keep only
  # elements of self NOT present in other (deduplicated).
  
  # int_array
  puts [1, 2, 3, 4].difference([2, 4]).inspect
  puts [1, 2, 3].difference([4, 5]).inspect
  puts [1, 2, 3].difference([1, 2, 3]).inspect
  puts [].difference([1, 2]).inspect
  puts [1, 2].difference([]).inspect
  puts [1, 1, 2, 3].difference([1]).inspect
  puts [].difference([]).inspect
  
  # str_array
  puts ["a", "b", "c"].difference(["b"]).inspect
  puts ["x", "y"].difference(["a"]).inspect
  puts ["a", "b"].difference(["a", "b"]).inspect
  puts ["a", "a", "b"].difference(["a"]).inspect
  
  # float_array
  puts [1.0, 2.0, 3.0].difference([2.0]).inspect
  puts [1.5, 2.5].difference([3.5]).inspect
  puts [1.0, 1.0, 2.0].difference([1.0]).inspect
  
  # sym_array
  puts [:a, :b, :c].difference([:b]).inspect
  puts [:x, :y].difference([:a]).inspect
  puts [:a, :a, :b].difference([:a]).inspect
end
t_array_difference

# === array_intersection ===
def t_array_intersection
  # int_array
  a = [1, 2, 3, 4]
  b = [3, 4, 5, 6]
  puts a.intersection(b).inspect
  
  # no common elements -> empty
  puts [1, 2].intersection([3, 4]).inspect
  
  # all in common
  puts [1, 2, 3].intersection([1, 2, 3]).inspect
  
  # duplicates in self are deduplicated
  puts [1, 1, 2, 3].intersection([1, 2]).inspect
  
  # empty self -> empty
  puts [].intersection([1, 2, 3]).inspect
  
  # empty other -> empty
  puts [1, 2, 3].intersection([]).inspect
  
  # both empty
  puts [].intersection([]).inspect
  
  # single element match
  puts [42].intersection([42]).inspect
  
  # single element no match
  puts [42].intersection([99]).inspect
  
  # str_array
  x = "a b c d".split(" ")
  y = "c d e f".split(" ")
  puts x.intersection(y).inspect
  
  # str no common -> empty
  puts "a b".split(" ").intersection("c d".split(" ")).inspect
  
  # str all common
  puts "x y".split(" ").intersection("x y".split(" ")).inspect
  
  # str empty self
  puts "".split(" ").intersection("a b".split(" ")).inspect
  
  # str duplicates in self are deduplicated
  puts "a a b c".split(" ").intersection("a b".split(" ")).inspect
  
  # str empty other -> empty
  puts "a b".split(" ").intersection("".split(" ")).inspect
  
  # float_array
  puts [1.0, 2.0, 3.0].intersection([2.0, 3.0, 4.0]).inspect
  
  # float no common -> empty
  puts [1.1, 2.2].intersection([3.3, 4.4]).inspect
  
  # float all common
  puts [1.5, 2.5].intersection([1.5, 2.5]).inspect
  
  # float duplicates in self are deduplicated
  puts [1.0, 1.0, 2.0].intersection([1.0]).inspect
  
  # float single no match -> empty
  puts [9.9].intersection([1.0, 2.0]).inspect
  
  # sym_array
  puts [:a, :b, :c, :d].intersection([:c, :d, :e]).inspect
  
  # sym no common -> empty
  puts [:a, :b].intersection([:c, :d]).inspect
  
  # sym all common
  puts [:x, :y].intersection([:x, :y]).inspect
  
  # sym duplicates in self are deduplicated
  puts [:a, :a, :b].intersection([:a]).inspect
end
t_array_intersection

# === array_union ===
def t_array_union
  # Array#union for typed arrays (int/sym/str/float).
  # Mirrors Array#intersection (c31b618). Returns a new array with all
  # unique elements from `self` followed by unique elements from `other`.
  
  # int_array
  puts [1, 2, 3].union([3, 4, 5]).inspect
  puts [1, 2].union([3, 4]).inspect
  puts [1, 2, 3].union([1, 2, 3]).inspect
  puts [].union([1, 2]).inspect
  puts [1, 2].union([]).inspect
  puts [1, 1, 2].union([2, 3]).inspect
  puts [].union([]).inspect
  
  # str_array
  puts ["a", "b"].union(["b", "c"]).inspect
  puts ["x"].union(["y", "z"]).inspect
  puts ["a", "b", "c"].union(["a", "b", "c"]).inspect
  puts ["a", "a", "b"].union(["b", "c"]).inspect
  
  # float_array
  puts [1.0, 2.0].union([2.0, 3.0]).inspect
  puts [1.5, 2.5].union([3.5]).inspect
  puts [1.0, 1.0, 2.0].union([2.0, 3.0]).inspect
  
  # sym_array
  puts [:a, :b].union([:b, :c]).inspect
  puts [:x].union([:y, :z]).inspect
  puts [:a, :a, :b].union([:b, :c]).inspect
end
t_array_union

