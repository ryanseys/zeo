# Bundled tests:
#   - array2
#   - array3
#   - array_difference_dups
#   - array_fill
#   - array_lit
#   - array_lit_unbox_poly
#   - array_mul
#   - array_new_block
#   - array_of_array

# === array2 ===
def t_array2
  # Test additional Array methods
  
  arr = (1..10).to_a
  
  # reject
  odds = arr.reject do |x|
    x % 2 == 0
  end
  puts odds.length  # 5
  
  # reduce/inject via each
  total = 0
  arr.each do |x|
    total += x
  end
  puts total  # 55
  
  # reverse
  nums = (1..10).to_a
  nums.reverse!
  puts nums[0]  # 10
  puts nums[9]  # 1
  
  # Array#first / Array#last
  puts nums.first  # 10
  puts nums.last   # 1
  
  # Array#min / Array#max
  mn = nums[0]
  mx = nums[0]
  nums.each do |x|
    if x < mn
      mn = x
    end
    if x > mx
      mx = x
    end
  end
  puts mn  # 1
  puts mx  # 10
  
  # Array#include?
  if nums.include?(5)
    puts "true"
  else
    puts "false"
  end   # true
  if nums.include?(11)
    puts "true"
  else
    puts "false"
  end  # false
  
  # Array#compact (remove nils) - simplified: just test count
  puts nums.length  # 10
end
t_array2

# === array3 ===
def t_array3
  # Test additional array methods
  
  arr = Array.new
  arr.push(5); arr.push(3); arr.push(8); arr.push(1)
  arr.push(4); arr.push(2); arr.push(7); arr.push(6)
  
  # count with block
  puts arr.count { |x| x > 4 }   # 4 (5,8,7,6)
  
  # count without block
  puts arr.count                   # 8
  
  # min_by / max_by
  puts arr.min_by { |x| x }       # 1
  puts arr.max_by { |x| x }       # 8
  
  # sort_by
  sorted = arr.sort_by { |x| -x }
  puts sorted[0]    # 8
  puts sorted[1]    # 7
  puts sorted[7]    # 1
  
  # StrArray count
  words = "hello world foo bar".split(" ")
  puts words.count { |w| w.length > 3 }  # 2 (hello, world)
  puts words.count                         # 4
  
  puts "done"
end
t_array3

# === array_difference_dups ===
def t_array_difference_dups
  # Array#- / Array#difference must preserve LHS duplicates that
  # aren't in the RHS. CRuby semantics:
  #   [1, 1, 2, 3] - [3]  → [1, 1, 2]   (NOT [1, 2])
  #   [1, 1, 2, 3] - [1]  → [2, 3]      (every 1 removed)
  
  # int_array
  puts ([1, 1, 2, 3] - [3]).inspect
  puts ([1, 1, 2, 3] - [1]).inspect
  puts ([1, 1, 1, 2, 2] - [1]).inspect
  puts ([1, 2, 1, 3, 1] - [3]).inspect
  
  # str_array
  puts (["a", "a", "b", "c"] - ["c"]).inspect
  puts (["a", "b", "a", "c", "a"] - ["c"]).inspect
  
  # float_array
  puts ([1.0, 1.0, 2.0, 3.0] - [3.0]).inspect
  puts ([1.5, 1.5, 2.5] - [2.5]).inspect
  
  # method form
  puts [1, 1, 2, 3].difference([3]).inspect
  puts ["a", "a", "b"].difference(["b"]).inspect
end
t_array_difference_dups

# === array_fill ===
def t_array_fill
  # Array#fill: 1-arg, 2-arg, 3-arg forms.
  # Previously the 2-/3-arg forms silently ignored start/length and
  # filled the entire array.
  
  # 1-arg: fill all
  a = [1, 2, 3, 4, 5]
  a.fill(9)
  puts a[0]    # 9
  puts a[4]    # 9
  
  # 2-arg: fill from start to end
  b = [1, 2, 3, 4, 5]
  b.fill(9, 2)
  puts b[0]    # 1
  puts b[1]    # 2
  puts b[2]    # 9
  puts b[4]    # 9
  
  # 3-arg: fill from start, length elements
  c = [1, 2, 3, 4, 5]
  c.fill(0, 1, 3)
  puts c[0]    # 1
  puts c[1]    # 0
  puts c[3]    # 0
  puts c[4]    # 5
  
  # Negative start
  d = [1, 2, 3, 4, 5]
  d.fill(7, -2)
  puts d[0]    # 1
  puts d[2]    # 3
  puts d[3]    # 7
  puts d[4]    # 7
  
  # 3-arg with start beyond length: array grows.
  # CRuby fills the gap with nil; Spinel's IntArray can't hold nil so it
  # uses 0. The grown length and the explicit fill values are the same;
  # we only assert on those (skip e[3]/e[4] which differ in formatting).
  e = [1, 2, 3]
  e.fill(9, 5, 2)
  puts e.length   # 7
  puts e[2]       # 3
  puts e[5]       # 9
  puts e[6]       # 9
  
  # 2-arg with start beyond length: no-op, array unchanged.
  f = [1, 2, 3]
  f.fill(9, 5)
  puts f.length   # 3
  puts f[2]       # 3
  
  # Very-negative start: clamped to 0 after wrap (start + len < 0).
  # CRuby: [1,2,3].fill(9, -5, 2) #=> [9, 9, 3]
  g = [1, 2, 3]
  g.fill(9, -5, 2)
  puts g[0]       # 9
  puts g[1]       # 9
  puts g[2]       # 3
end
t_array_fill

# === array_lit ===
def t_array_lit
  # Test array literals as IntArray/StrArray
  
  # Integer array literal
  arr = [5, 3, 8, 1, 4]
  puts arr.length    # 5
  puts arr.size      # 5  (alias for length)
  puts arr[0]        # 5
  puts arr.sort[0]   # 1
  puts arr.sum       # 21
  puts arr.min       # 1
  puts arr.max       # 8
  
  # IntArray.size after mutation
  arr.push(99)
  puts arr.size      # 6  (length+1 after push)
  arr.pop
  puts arr.size      # 5  (length-1 after pop)
  
  # Symbol array literal -- size shares the same dispatch as IntArray
  syms = [:foo, :bar, :baz]
  puts syms.length   # 3
  puts syms.size     # 3
  
  # String array literal
  words = ["hello", "world", "foo"]
  puts words.length  # 3
  puts words.size    # 3
  puts words[0]      # hello
  puts words.join(", ") # hello, world, foo
  
  # Empty array
  empty = []
  empty.push(42)
  puts empty.length  # 1
  puts empty.size    # 1
  puts empty[0]      # 42
  
  # Hoisted-length optimisation: .size should behave the same as
  # .length when used as a loop bound. Both compile to a single
  # read of the array's len field, hoisted out of the loop body.
  sum = 0
  i = 0
  while i < arr.size
    sum += arr[i]
    i += 1
  end
  puts sum           # 21
  
  ssum = 0
  si = 0
  while si < syms.size
    ssum += 1
    si += 1
  end
  puts ssum          # 3
  
  wlen = 0
  wi = 0
  while wi < words.size
    wlen += words[wi].length
    wi += 1
  end
  puts wlen          # 13
  
  puts "done"
end
t_array_lit

# === array_lit_unbox_poly ===
def t_array_lit_unbox_poly
  # `compile_array_literal`'s int-array fallback pushed elements
  # verbatim, including poly-typed ones. When a local was widened to
  # poly via cross-branch assignments and then wrapped in an `[x]`
  # literal, the generated C had `sp_IntArray_push(arr, <sp_RbVal
  # struct>)`, which gcc rejected with "incompatible type for
  # argument 2 of sp_IntArray_push". The fix unboxes the int payload
  # via `.v.i`. Caller code is responsible for keeping the poly slot
  # integer-tagged at the moment the literal is built.
  
  addr = 7
  flag = 0
  if flag > 0
    # Dead path at runtime — only here to widen `addr` to poly so
    # the array literal lands on the unboxing path.
    addr = "x"
  end
  
  # `addr` is poly at compile time (sp_RbVal) but always int=7 at
  # runtime. The `[addr]` literal infers as int_array (a single poly
  # element falls through to the IntArray branch).
  arr = [addr]
  puts arr.length          # 1
  puts arr[0]              # 7
end
t_array_lit_unbox_poly

# === array_mul ===
def t_array_mul
  # Array#* (repeat). Works uniformly across every typed array kind
  # is_array_type() recognises (int / float / str / sym / poly).
  
  # IntArray
  ints = [1, 2, 3] * 3
  puts ints.length      # 9
  puts ints[0]          # 1
  puts ints[3]          # 1
  puts ints[8]          # 3
  
  # IntArray * 0 → empty
  empty = [1, 2, 3] * 0
  puts empty.length     # 0
  
  # IntArray * 1 → copy (independent of source)
  arr = [1, 2]
  copy = arr * 1
  arr.push(99)
  puts copy.length      # 2
  
  # FloatArray
  floats = [1.5, 2.5] * 2
  puts floats.length    # 4
  puts floats[0]        # 1.5
  puts floats[3]        # 2.5
  
  # StrArray
  strs = ["a", "b"] * 3
  puts strs.length      # 6
  puts strs[0]          # a
  puts strs[5]          # b
  
  # SymArray
  syms = [:x, :y] * 2
  puts syms.length      # 4
  puts syms[0]          # x
  puts syms[3]          # y
  
  # PolyArray (mixed types)
  mixed = [1, "two", 3.0] * 2
  puts mixed.length     # 6
  
  # Pre-fill an array
  zeros = [0] * 5
  puts zeros.length     # 5
  puts zeros[0]         # 0
  puts zeros[4]         # 0
  
  puts "done"
end
t_array_mul

# === array_new_block ===
def t_array_new_block
  # Array.new(N) { |i| ... }: block form with index parameter.
  # Previously returned an empty IntArray (the block was ignored).
  
  # Block uses index
  a = Array.new(5) { |i| i * 2 }
  puts a.length      # 5
  puts a[0]          # 0
  puts a[2]          # 4
  puts a[4]          # 8
  puts a.sum         # 20
  
  # Block returns constant
  b = Array.new(3) { 42 }
  puts b.length      # 3
  puts b[0]          # 42
  puts b[2]          # 42
  
  # Zero-length
  c = Array.new(0) { 99 }
  puts c.length      # 0
end
t_array_new_block

# === array_of_array ===
def t_array_of_array
  # Array of arrays (2-level nesting)
  a = [[1, 2], [3, 4], [5, 6]]
  puts a.length
  
  # Iterate nested arrays
  a.each { |sub|
    puts sub[0] + sub[1]
  }
  
  # String array of arrays
  b = [["hello", "world"], ["foo", "bar"]]
  b.each { |pair|
    puts pair.join(" ")
  }
  
  # Push to array of arrays
  c = [[10, 20]]
  c.push([30, 40])
  c.push([50, 60])
  puts c.length
  c.each { |row|
    row.each { |v| print v.to_s + " " }
    puts ""
  }
end
t_array_of_array

