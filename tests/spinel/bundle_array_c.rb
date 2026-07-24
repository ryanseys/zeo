# Bundled tests:
#   - array_splat_range
#   - array_take_drop_while
#   - array_tally_sym
#   - array_transpose
#   - empty_poly_array_reassignment

# === array_splat_range ===
def t_array_splat_range
  # `[*0..n]` and `[*arr]` are array literals with a SplatNode element.
  # Without SplatNode handling in compile_array_literal, the splat was
  # lowered to a single default value (e.g., just the range's lower
  # bound), so `[*0..4096]` produced a 1-element array. This broke
  # optcarrot's dummy palette and made every `palette[i]` for i > 0
  # read out-of-bounds garbage.
  
  a = [*0..5]
  puts a.length
  puts a[0]
  puts a[3]
  puts a[5]
  
  b = [*0...4]
  puts b.length
  puts b[0]
  puts b[3]
  
  src = [10, 20, 30]
  c = [*src]
  puts c.length
  puts c[0]
  puts c[2]
end
t_array_splat_range

# === array_take_drop_while ===
def t_array_take_drop_while
  # Array#take_while and Array#drop_while for int_array (the common case).
  # take_while collects elements from the front while the block stays
  # truthy; drop_while skips them and returns the rest.
  
  # take_while
  puts [1, 2, 3, 1].take_while { |x| x < 3 }.inspect
  puts [1, 2, 3].take_while { |x| x < 10 }.inspect
  puts [5, 6, 7].take_while { |x| x < 0 }.inspect
  puts [].take_while { |x| x > 0 }.inspect
  
  # drop_while
  puts [1, 2, 3, 1].drop_while { |x| x < 3 }.inspect
  puts [1, 2, 3].drop_while { |x| x < 10 }.inspect
  puts [5, 6, 7].drop_while { |x| x < 0 }.inspect
  puts [].drop_while { |x| x > 0 }.inspect
  
  # take_while + drop_while round-trip preserves total count
  arr = [1, 2, 3, 4, 5, 1, 2]
  puts(arr.take_while { |x| x < 4 }.length + arr.drop_while { |x| x < 4 }.length)
  
  # Multi-stmt block — preceding statements must execute (regression for the
  # "only last expr is compiled" bug).
  counter = 0
  [1, 2, 3, 4].take_while { |x| counter = counter + 1; x < 3 }
  puts counter
  
  # sym_array path (regression for the bp-hardcoded-int bug).
  puts [:a, :b, :c, :d].take_while { |s| s != :c }.inspect
  puts [:a, :b, :c, :d].drop_while { |s| s != :c }.inspect
end
t_array_take_drop_while

# === array_tally_sym ===
def t_array_tally_sym
  # Array#tally for sym_array. str_array#tally already shipped — this
  # extends it to symbol arrays via a sp_SymArray_tally runtime helper
  # that produces a sym_int_hash mapping each unique element to its
  # occurrence count.
  
  # Basic tally over symbol array
  puts [:a, :b, :a, :c, :a, :b].tally[:a]
  puts [:a, :b, :a, :c, :a, :b].tally[:b]
  puts [:a, :b, :a, :c, :a, :b].tally[:c]
  
  # Single element
  puts [:foo].tally[:foo]
  
  # All same
  puts [:x, :x, :x, :x].tally[:x]
  
  # Length of result
  puts [:a, :b, :a, :c, :a, :b].tally.length
  puts [:foo].tally.length
  puts [:x, :x, :x, :x].tally.length
  
  # has_key? confirms membership
  puts [:a, :b, :c].tally.has_key?(:a)
  puts [:a, :b, :c].tally.has_key?(:z)
end
t_array_tally_sym

# === array_transpose ===
def t_array_transpose
  # Issue #156: transpose for nested int arrays.
  # Inspect of nested arrays still prints "#<Object>" (separate issue),
  # so each test reads element-wise to keep the comparison meaningful.
  
  m = [[1, 2], [3, 4]]
  t = m.transpose
  puts t.length
  puts t[0][0]
  puts t[0][1]
  puts t[1][0]
  puts t[1][1]
  
  # 2x3 -> 3x2
  m2 = [[1, 2, 3], [4, 5, 6]]
  t2 = m2.transpose
  puts t2.length
  puts t2[0][0]
  puts t2[0][1]
  puts t2[1][0]
  puts t2[1][1]
  puts t2[2][0]
  puts t2[2][1]
  
  # 1x3 -> 3x1
  m3 = [[7, 8, 9]]
  t3 = m3.transpose
  puts t3.length
  puts t3[0][0]
  puts t3[1][0]
  puts t3[2][0]
end
t_array_transpose

# === empty_poly_array_reassignment ===
def t_empty_poly_array_reassignment
  items = [0, ""]
  items = []
  items.push(1)
  items.push("x")
  
  puts items.length
  puts items[0]
  puts items[1]
end
t_empty_poly_array_reassignment

