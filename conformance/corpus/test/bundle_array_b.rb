# Bundled tests:
#   - array_ptr_array_inspect
#   - array_push
#   - array_range
#   - array_replace
#   - array_reverse_bang
#   - array_set_ops
#   - array_shovel
#   - array_shuffle_sym_poly
#   - array_slice_assign
#   - array_sort_bang

# === array_ptr_array_inspect ===
def t_array_ptr_array_inspect
  # Issue #169: p / inspect on nested arrays whose element type is one
  # of the four built-in T_array shapes (int/str/float/sym).
  
  p [[1, 2], [3, 4]]
  p [[1, 2, 3], [4, 5, 6]]
  p [[10]]
  p [[1, 2], [3, 4]].transpose
  
  p [["a", "b"], ["c", "d"]]
  p [["hello", "world"]]
  
  p [[1.5, 2.5], [3.5, 4.5]]
  p [[0.1]]
  
  p [[:a, :b], [:c, :d]]
  p [[:foo]]
end
t_array_ptr_array_inspect

# === array_push ===
def t_array_push
  # Array#push across typed-array variants.
  # Previously push only dispatched for IntArray and StrArray; FloatArray
  # and SymArray (and PtrArray) silently fell through to a no-op expression.
  
  ints = [1, 2, 3]
  ints.push(4)
  ints.push(5)
  puts ints.length          # 5
  puts ints[3]              # 4
  puts ints[4]              # 5
  
  # Float values use non-integer fractional parts so Spinel's float-puts
  # (which strips a trailing ".0") matches CRuby's output.
  floats = [1.5, 2.5]
  floats.push(3.5)
  floats.push(4.25)
  puts floats.length        # 4
  puts floats[2]            # 3.5
  puts floats[3]            # 4.25
  
  strs = ["a", "b"]
  strs.push("c")
  strs.push("d")
  puts strs.length          # 4
  puts strs[2]              # c
  puts strs[3]              # d
  
  syms = [:x, :y]
  syms.push(:z)
  puts syms.length          # 3
  puts syms[2]              # z
  
  # << on every typed-array variant. Previously << did not dispatch
  # for sym_array, so `syms << :z` silently fell through.
  ints2 = [10]
  ints2 << 20
  ints2 << 30
  puts ints2.length         # 3
  puts ints2[2]             # 30
  
  floats2 = [1.5]
  floats2 << 2.5
  puts floats2.length       # 2
  puts floats2[1]           # 2.5
  
  strs2 = ["x"]
  strs2 << "y"
  puts strs2.length         # 2
  puts strs2[1]             # y
  
  syms2 = [:p]
  syms2 << :q
  puts syms2.length         # 2
  puts syms2[1]             # q
end
t_array_push

# === array_range ===
def t_array_range
  # IntArray slicing: a[range] and a[start, len].
  # Previously a[1..2] failed with `incompatible type for argument 2 of
  # 'sp_IntArray_get'` (Range passed where mrb_int was expected), and
  # a[1, 2] silently dropped the second arg and returned a single int.
  
  a = [10, 20, 30, 40, 50]
  
  # Range form
  b = a[1..3]
  puts b.length      # 3
  puts b[0]          # 20
  puts b[1]          # 30
  puts b[2]          # 40
  
  # (start, len) form
  c = a[1, 2]
  puts c.length      # 2
  puts c[0]          # 20
  puts c[1]          # 30
  
  # Negative start (counts from end)
  d = a[-2, 2]
  puts d.length      # 2
  puts d[0]          # 40
  puts d[1]          # 50
  
  # len exceeds remaining: clamped
  f = a[2, 100]
  puts f.length      # 3
  puts f[0]          # 30
  puts f[2]          # 50
  
  # Range to last index
  g = a[2..4]
  puts g.length      # 3
  puts g[0]          # 30
  puts g[2]          # 50
  
  # Bare a[i] still works as scalar get
  puts a[0]          # 10
  puts a[-1]         # 50
  
  # Result is usable as an IntArray
  puts a[1..3].sum   # 90
end
t_array_range

# === array_replace ===
def t_array_replace
  # Array#replace: mutating in-place copy from another array.
  # Previously a silent no-op for arrays; only String/MutableString worked.
  
  a = [1, 2, 3, 4, 5]
  a.replace([10, 20, 30])
  puts a.length    # 3
  puts a[0]        # 10
  puts a[2]        # 30
  
  # Replace into smaller -> larger triggers grow
  b = [1]
  b.replace([100, 200, 300, 400, 500, 600, 700, 800, 900])
  puts b.length    # 9
  puts b[8]        # 900
  
  # String array
  s = ["a", "b", "c"]
  s.replace(["x", "y"])
  puts s.length    # 2
  puts s[0]        # x
  puts s[1]        # y
  
  # Float array
  f = [1.0, 2.0, 3.0]
  f.replace([7.5, 8.5])
  puts f.length    # 2
  puts f[0]        # 7.5
  puts f[1]        # 8.5
  
  # Mutable string (existing path; <<-promoted)
  ms = ""
  ms << "hello"
  ms.replace("world")
  puts ms          # world
end
t_array_replace

# === array_reverse_bang ===
def t_array_reverse_bang
  # Array#reverse! across typed-array variants.
  # Previously only IntArray dispatched; SymArray, StrArray, FloatArray
  # silently fell through.
  
  ints = [1, 2, 3, 4, 5]
  ints.reverse!
  ints.each { |i| puts i }   # 5 4 3 2 1
  
  floats = [1.5, 2.5, 3.5, 4.25]
  floats.reverse!
  floats.each { |f| puts f }  # 4.25 3.5 2.5 1.5
  
  strs = ["alpha", "beta", "gamma"]
  strs.reverse!
  strs.each { |s| puts s }    # gamma beta alpha
  
  syms = [:a, :b, :c, :d]
  syms.reverse!
  syms.each { |y| puts y }    # d c b a
  
  # Even-length and single-element edge cases
  even = [10, 20]
  even.reverse!
  even.each { |i| puts i }   # 20 10
  
  one = [42]
  one.reverse!
  one.each { |i| puts i }    # 42
  
  empty = []
  empty.reverse!
  puts empty.length          # 0
end
t_array_reverse_bang

# === array_set_ops ===
def t_array_set_ops
  # Array#&, Array#-, Array#| all dispatch to typed-array intersect /
  # difference / union helpers. Each operator's per-shape coverage
  # (int / str / float / sym / variable receiver / chained) is
  # parallel — one file per operator was duplicating the matrix.
  # Element semantics live in test/array_{intersection,difference,union}.rb;
  # this file only exercises the operator dispatch path.
  
  # &
  puts ([1, 2, 3, 4] & [3, 4, 5]).inspect
  puts (["a", "b", "c"] & ["b", "c"]).inspect
  puts ([1.0, 2.0, 3.0] & [2.0, 3.0]).inspect
  puts ([:a, :b, :c] & [:b, :c]).inspect
  amp = [1, 2, 3, 4]
  puts (amp & [3, 4, 5]).inspect
  puts ([1, 2, 3, 4] & [2, 3, 4] & [3, 4]).inspect
  
  # -
  puts ([1, 2, 3, 4] - [2, 4]).inspect
  puts (["a", "b", "c"] - ["b"]).inspect
  puts ([1.0, 2.0, 3.0] - [2.0]).inspect
  puts ([:a, :b, :c] - [:b]).inspect
  mns = [1, 2, 3, 4]
  puts (mns - [2, 4]).inspect
  puts ([1, 2, 3, 4, 5] - [2] - [4]).inspect
  
  # |
  puts ([1, 2, 3] | [3, 4, 5]).inspect
  puts (["a", "b"] | ["b", "c"]).inspect
  puts ([1.0, 2.0] | [2.0, 3.0]).inspect
  puts ([:a, :b] | [:b, :c]).inspect
  pip = [1, 2, 3]
  puts (pip | [3, 4]).inspect
  puts ([1, 2] | [2, 3] | [3, 4]).inspect
end
t_array_set_ops

# === array_shovel ===
def t_array_shovel
  # `arr << x` used in expression context (not a top-level statement)
  # fell through to a literal C `<<` (bit shift). The stmt-level path
  # already lowered `arr << x` to push for typed arrays; the operator/
  # expression form did not. gcc rejected the result with "invalid
  # operands to binary <<" on a pointer LHS.
  #
  # Chaining `(arr << x) << y` is the natural expression-context use
  # case: the inner `<<` returns the recv, which the outer `<<`
  # mutates again. Both the codegen path and `infer_call_type` need
  # to know the result is the recv's array type so the outer operand
  # type-checks.
  
  ints = [1]
  (ints << 2) << 3
  puts ints.length    # 3
  puts ints[0]        # 1
  puts ints[1]        # 2
  puts ints[2]        # 3
  
  floats = [1.5]
  (floats << 2.5) << 3.5
  puts floats.length  # 3
  puts floats[2]      # 3.5
  
  strs = ["a"]
  (strs << "b") << "c"
  puts strs.length    # 3
  puts strs[2]        # c
  
  syms = [:x]
  (syms << :y) << :z
  puts syms.length    # 3
  puts syms[2]        # z
end
t_array_shovel

# === array_shuffle_sym_poly ===
def t_array_shuffle_sym_poly
  # Array#shuffle / Array#shuffle! used to skip sym_array and poly_array.
  # For sym_array the dispatcher silently fell through (bin output empty);
  # for poly_array the runtime had no shuffle helper at all.
  
  # sym_array
  sa = [:a, :b, :c, :d].shuffle
  puts sa.length
  
  # poly_array
  pa = [1, "a", :s, 2.0].shuffle
  puts pa.length
end
t_array_shuffle_sym_poly

# === array_slice_assign ===
def t_array_slice_assign
  # `arr[i, n] = src` (Array#[]= with three args) replaces n
  # elements of `arr` starting at index i with the elements of
  # `src`. Same-length only — n must equal `src.length`; resize
  # semantics not supported.
  #
  # Without this, the codegen path for `[]=` only looked at args
  # 0 and 1, so `a[2, 8] = [10, 20, ...]` lowered to `a[2] = 8`,
  # silently treating the length argument as the value.
  
  a = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
  a[2, 8] = [10, 20, 30, 40, 50, 60, 70, 80]
  p a
  
  # Float array
  f = [0.0, 0.0, 0.0, 0.0]
  f[1, 2] = [3.5, 4.5]
  p f
  
  # String array
  s = ["", "", "", ""]
  s[0, 3] = ["a", "b", "c"]
  p s
end
t_array_slice_assign

# === array_sort_bang ===
def t_array_sort_bang
  # Array#sort! across typed-array variants.
  # Previously only IntArray and SymArray dispatched; StrArray and
  # FloatArray silently fell through.
  
  ints = [3, 1, 4, 1, 5, 9, 2, 6]
  ints.sort!
  ints.each { |i| puts i }   # 1 1 2 3 4 5 6 9
  
  floats = [3.5, 1.25, 4.75, 1.5, 0.25]
  floats.sort!
  floats.each { |f| puts f }  # 0.25 1.25 1.5 3.5 4.75
  
  strs = ["banana", "apple", "cherry", "date"]
  strs.sort!
  strs.each { |s| puts s }    # apple banana cherry date
  
  syms = [:c, :a, :d, :b]
  syms.sort!
  syms.each { |y| puts y }    # a b c d
  
  # Single-element and empty edge cases
  one = [99]
  one.sort!
  one.each { |i| puts i }     # 99
  
  empty = []
  empty.sort!
  puts empty.length           # 0
end
t_array_sort_bang

