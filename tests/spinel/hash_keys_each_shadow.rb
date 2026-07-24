# Follow-up to PR #120: the hash.keys.each fusion path emitted the
# block param as a bare assignment (`lv_k = hash->order[i]`), so a
# block param that shadowed an outer same-named local of a different
# type re-used the outer local's C declaration and the assignment
# warned / produced wrong values. Mirror PR #115's pattern: declare
# `lv_<bp>` inline with the key's C type and wrap the body in
# push_scope / declare_var / pop_scope.

# 1. Outer string `k`, inner int key (int_str_hash).
def f1
  k = "outer-string"
  h = {1 => "a", 2 => "b"}
  h.keys.each do |k|
    puts k * 10
  end
  puts k
end
f1

# 2. Outer int `s`, inner string key (str_int_hash).
def f2
  s = 42
  h = {"x" => 1, "y" => 2}
  h.keys.each do |s|
    puts s + "!"
  end
  puts s
end
f2

# 3. Outer string `n`, inner sym key (sym_int_hash).
def f3
  n = "outer"
  h = {a: 10, b: 20}
  h.keys.each do |n|
    puts n
  end
  puts n
end
f3
