# Bundled tests:
#   - int_array_each_with_index
#   - int_array_each_with_object
#   - int_array_reduce
#   - int_array_reject
#   - int_array_select
#   - int_array_zip

# === int_array_each_with_index ===
def t_int_array_each_with_index
  # Array#each_with_index on an int_array. The element block param must
  # shadow an outer same-named local of a different C type, and the
  # index block param must be block-local mrb_int (not leak the outer
  # `j`). Body skips reading the params to keep this scope-shadow case
  # separate from the format-specifier path.
  
  i = "hi"
  j = "ho"
  puts i
  puts j
  foo = [10, 20, 30]
  n = 0
  foo.each_with_index { |i, j| n = n + 1 }
  puts n   # 3
end
t_int_array_each_with_index

# === int_array_each_with_object ===
def t_int_array_each_with_object
  # Array#each_with_object on an int_array. The element block param
  # must shadow an outer same-named local of a different C type, and
  # the accumulator block param must take the C type of the seed
  # (here mrb_int) — not leak the outer `acc` (string). Body avoids
  # reading the params to keep this scope-shadow case separate from
  # the separate type-inferer behavior when reading the param inside
  # the block.
  
  i = "hi"
  acc = "ho"
  puts i
  puts acc
  foo = [10, 20, 30]
  n = 0
  foo.each_with_object(0) { |i, acc| n = n + 1 }
  puts n   # 3
end
t_int_array_each_with_object

# === int_array_reduce ===
def t_int_array_reduce
  # Array#inject and Array#reduce share compile_reduce_block but have
  # distinct dispatch entries. Cover both so a divergence at either
  # entry surfaces. The block param `i` must also shadow an outer
  # same-named local of a different C type without leaking back out.
  
  i = "hi"
  puts i
  
  foo = [1, 2, 3, 4, 5]
  sum_inject = foo.inject(0) { |acc, i| acc + i }
  puts sum_inject   # 15
  
  sum_reduce = foo.reduce(0) { |acc, i| acc + i }
  puts sum_reduce   # 15
end
t_int_array_reduce

# === int_array_reject ===
def t_int_array_reject
  # Array#reject on an int_array. The block param must shadow an outer
  # same-named local of a different C type. Body uses a constant
  # predicate so this isolates the shadow case from the separate
  # type-inferer behavior when reading the param inside the block.
  
  i = "hi"
  puts i
  foo = [10, 20, 30]
  none = foo.reject { |i| true }
  puts none.length  # 0
  all = foo.reject { |i| false }
  puts all.length   # 3
  puts all[0]       # 10
  puts all[1]       # 20
  puts all[2]       # 30
end
t_int_array_reject

# === int_array_select ===
def t_int_array_select
  # Array#select on an int_array. The block param must shadow an outer
  # same-named local of a different C type. Body uses a constant
  # predicate so this isolates the shadow case from the separate
  # type-inferer behavior when reading the param inside the block.
  
  i = "hi"
  puts i
  foo = [10, 20, 30]
  all = foo.select { |i| true }
  puts all.length   # 3
  puts all[0]       # 10
  puts all[1]       # 20
  puts all[2]       # 30
  none = foo.select { |i| false }
  puts none.length  # 0
end
t_int_array_select

# === int_array_zip ===
def t_int_array_zip
  # Array#zip with a block on int_arrays. Both block params must shadow
  # outer same-named locals of a different C type. Body uses a counter
  # so this isolates the scope-shadow case from any param-read path.
  
  a = "ha"
  b = "hb"
  puts a
  puts b
  foo = [1, 2, 3]
  bar = [10, 20, 30]
  n = 0
  foo.zip(bar) { |a, b| n = n + 1 }
  puts n   # 3
end
t_int_array_zip

