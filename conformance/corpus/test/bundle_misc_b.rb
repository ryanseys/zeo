# Bundled tests:
#   - local_or_and_write
#   - multi_target_nested
#   - numbered_block_params
#   - numbered_params_destructure
#   - parens_multi_stmt_and_local_write_expr
#   - sort_reduce
#   - gc_root_map_accumulator
#   - gc_root_range_and_times_map
#   - transpose_gc_root
#   - utf8

# === local_or_and_write ===
def t_local_or_and_write
  # `LocalVariableOrWriteNode` (`a ||= b`) and
  # `LocalVariableAndWriteNode` (`a &&= b`) parse as their own
  # AST nodes — distinct from `LocalVariableOperatorWriteNode`
  # (which carries `+`, `-`, `*`, etc. via a binary_operator field).
  # Without dedicated parser cases the prism nodes were dropped on
  # the floor, so `a ||= b` produced no C output.
  
  # (1) Statement-form ||=
  a = nil
  a ||= 10
  puts a       # 10
  a ||= 99     # already truthy → no-op
  puts a       # 10
  
  # (2) Statement-form &&=
  b = 5
  b &&= b + 1
  puts b       # 6
  c = nil
  c &&= 99     # nil → no-op
  puts c.nil? ? "nil" : c.to_s   # nil
  
  # (3) Expression-form ||=
  x = nil
  y = (x ||= 7)
  puts x       # 7
  puts y       # 7
  
  # (4) Expression-form &&=
  p = 3
  q = (p &&= p * 2)
  puts p       # 6
  puts q       # 6
end
t_local_or_and_write

# === multi_target_nested ===
def t_multi_target_nested
  # MultiTargetNode -- nested LHS in destructuring multi-assign.
  #
  #   a, (b, c), d = 1, [2, 3], 4
  #
  # Each parenthesized group on the LHS is a MultiTargetNode that
  # recursively unpacks its slot of the RHS. Spinel routes through
  # emit_multi_write_target which dispatches on the target node type.
  # The inner-array RHS slot must be a typed-array (int_array, str_array,
  # or float_array). Two-level nesting where intermediate slots are
  # heterogeneous (poly_array) is out of scope -- documented inline.
  
  a, (b, c), d = 1, [2, 3], 4
  puts a    # 1
  puts b    # 2
  puts c    # 3
  puts d    # 4
  
  # String-typed inner array
  x, (s, t), y = 10, ["hello", "world"], 20
  puts x    # 10
  puts s    # hello
  puts t    # world
  puts y    # 20
end
t_multi_target_nested

# === numbered_block_params ===
def t_numbered_block_params
  # Numbered block params (`_1`) and Ruby 3.4 implicit `it`.
  #
  # `_1` was already supported via Prism's NumberedParametersNode + a
  # regular LocalVariableReadNode at the use site. Implicit `it` (Ruby
  # 3.4) was emitted as PM_IT_PARAMETERS_NODE / PM_IT_LOCAL_VARIABLE_READ_NODE
  # which the codegen had no handler for. spinel_parse now lowers both
  # to their `_1` equivalents so the codegen reuses the existing path.
  
  # 1. `_1` over an int array — each.
  [1, 2, 3].each { puts _1 }
  # 1
  # 2
  # 3
  
  # 2. `_1` over a map+each chain.
  [10, 20].map { _1 * 2 }.each { puts _1 }
  # 20
  # 40
  
  # 3. `it` over an int array — each with arithmetic.
  [1, 2, 3].each { puts it * 2 }
  # 2
  # 4
  # 6
  
  # 4. `it` over a map+each chain.
  [10, 20, 30].map { it * 2 }.each { puts it }
  # 20
  # 40
  # 60
  
  # 5. `it` mixed with arithmetic and comparison.
  [1, 2, 3, 4].select { it > 2 }.each { puts it }
  # 3
  # 4
  
  # 6. `it` over a string array.
  ["alice", "bob"].each { puts it }
  # alice
  # bob
end
t_numbered_block_params

# === numbered_params_destructure ===
def t_numbered_params_destructure
  # Multi-arg numbered block params (`_1`, `_2`, ...) should destructure
  # the yielded sub-array. Pre-fix: `_1` binds to the whole element
  # (the sp_IntArray pointer) and `_2` is uninitialized -> "<ptr>=0".
  # CRuby ref: arity-N blocks over a single Array argument auto-destructure.
  
  # Plain int-tuple sub-arrays - max=2 destructure
  [[1, 10], [2, 20], [3, 30]].each { puts "#{_1}=#{_2}" }
  
  # Three-element sub-arrays exercise _3
  [[1, 2, 3], [10, 20, 30]].each { puts "#{_1}-#{_2}-#{_3}" }
  
  # Sum of paired elements via destructured slots in `.each`
  total = 0
  [[1, 100], [2, 200], [3, 300]].each { total = total + _1 + _2 }
  puts total
  
  # Two-stage: outer each with _1+_2, inner reuse outside the block
  running = 0
  [[10, 1], [20, 2], [30, 3]].each { running = running + _1 - _2 }
  puts running
  
  # Short sub-array regression — pre-fix the destructure read past the
  # sub-array's data buffer (OOB) when the yielded element was shorter
  # than the block's max numbered param. The fix bounds-checks each slot
  # read and pads with 0 (typed-nil analogue). This test computes the
  # sum of `_1 + _2` only when `_2` is not nil (so CRuby gets the same
  # numbers as Spinel — Spinel's typed-zero already passes the .nil?
  # false branch). `_2` is mentioned in the block so destruct_n >= 2.
  short_total = 0
  [[1], [2, 20], [3, 30]].each { short_total = short_total + _1 + (_2.nil? ? 0 : _2) }
  puts short_total
end
t_numbered_params_destructure

# === parens_multi_stmt_and_local_write_expr ===
def t_parens_multi_stmt_and_local_write_expr
  # Verify two expression-context constructs:
  # - `(stmt1; stmt2; ...; expr)` — leading statements run for
  #   side effects; the value of the parens is the last expression.
  # - `local = expr` and `local OP= expr` used as expressions —
  #   they assign and yield the new value of the local.
  
  # (1) Multi-stmt parens — leading side effects must run.
  x = 0
  y = (x = x + 1; x = x + 1; x)
  puts x      # 2
  puts y      # 2
  
  # (2) `local = expr` as the value of an outer expression.
  a = 0
  b = (a = 5)
  puts a      # 5
  puts b      # 5
  
  # (3) `local OP= expr` as the value of an outer expression.
  c = 10
  d = (c += 3)
  puts c      # 13
  puts d      # 13
end
t_parens_multi_stmt_and_local_write_expr

# === sort_reduce ===
def t_sort_reduce
  # Test Array#sort, reduce, min, max, sum
  
  arr = (1..10).to_a
  arr.reverse!
  
  # sort
  sorted = arr.sort
  puts sorted.first  # 1
  puts sorted.last   # 10
  
  # min/max/sum
  puts arr.min   # 1
  puts arr.max   # 10
  puts arr.sum   # 55
  
  # reduce
  puts arr.reduce(0) { |sum, x| sum + x }   # 55
  puts arr.reduce(1) { |prod, x| prod * x } # 3628800
  
  # inject (alias)
  puts arr.inject(0) { |s, x| s + x }  # 55
end
t_sort_reduce

# === gc_root_map_accumulator ===
def t_gc_root_map_accumulator
  # A nested map block — outer iteration allocates plenty of inner
  # objects via sp_*_new — can trigger a GC pass between pushes
  # into the outer accumulator. Without rooting the accumulator,
  # GC frees it as unreachable; the next push then writes into
  # freed memory and corrupts malloc bookkeeping, surfacing as a
  # SIGSEGV in _int_malloc on the next allocation.
  #
  # Each .map below crosses spinel's 256KB GC threshold mid-loop
  # (each block iteration allocates a discarded scratch string
  # alongside the kept result), so the outer accumulator must be
  # rooted to survive a collection.
  
  # (1) int_array recv → StrArray accumulator (string-return block).
  ints = []
  i = 0
  while i < 20000
    ints << i
    i += 1
  end
  
  r1 = ints.map do |x|
    _scratch = "scratch-#{x}-discarded-inner-allocation-padding"
    "kept-string-value-#{x}"
  end
  puts r1.length            # 20000
  puts r1[0]                # kept-string-value-0
  puts r1[19999][-3, 3]     # 999
  
  # (2) int_array recv → IntArray accumulator (int-return block).
  r2 = ints.map do |x|
    _scratch = "scratch-#{x}-discarded-inner-allocation-padding"
    x * 2
  end
  puts r2.length            # 20000
  puts r2[0]                # 0
  puts r2[19999]            # 39998
  
  # (3) str_array recv → StrArray accumulator (string-return block).
  strs = []
  i = 0
  while i < 20000
    strs << "src-#{i}"
    i += 1
  end
  
  r3 = strs.map do |s|
    _scratch = "scratch-#{s}-discarded-inner-allocation-padding"
    "out-#{s}"
  end
  puts r3.length            # 20000
  puts r3[0]                # out-src-0
  puts r3[19999][-3, 3]     # 999
  
  # (4) str_array recv → IntArray accumulator (int-return block).
  r4 = strs.map do |s|
    _scratch = "scratch-#{s}-discarded-inner-allocation-padding"
    s.length
  end
  puts r4.length            # 20000
  puts r4[0]                # 5  (out-0 → "src-0")
  puts r4[19999]            # 9  ("src-19999")
end
t_gc_root_map_accumulator

# === gc_root_range_and_times_map ===
def t_gc_root_range_and_times_map
  # `Range#map` and `N.times.map` build a fresh accumulator
  # (IntArray / StrArray / FloatArray) and push each block result.
  # When the block allocates inside (e.g. string interpolation), a
  # GC pass triggered mid-loop frees the unrooted accumulator and
  # the next push corrupts malloc bookkeeping — typically surfaces
  # as a SIGSEGV in `_int_malloc` on the next allocation.
  #
  # `int_array` / `str_array` recv branches already root the
  # accumulator (PR #198). This covers the missing branches:
  # `Range#map` and `N.times.map`.
  
  # (1) Range#map with string-allocating block.
  r1 = (0..3000).map { |i| "padded-string-#{i}-with-some-extra-text" }
  puts r1.length          # 3001
  puts r1[0][0, 6]        # padded
  puts r1[3000][-3, 3]    # ext
  
  # (2) Range#map with int block (heap pressure from inner allocations).
  r2 = (0...3000).map do |i|
    _scratch = "scratch-#{i}-discarded-inner-allocation-padding"
    i * 2
  end
  puts r2.length          # 3000
  puts r2[0]              # 0
  puts r2[2999]           # 5998
  
  # (3) N.times.map.
  r3 = 3000.times.map { |i| "kept-#{i}-with-extra-padding-text" }
  puts r3.length          # 3000
  puts r3[0]              # kept-0-with-extra-padding-text
  puts r3[2999][-3, 3]    # ext
  
  # (4) N.times.map with int block + scratch allocations.
  r4 = 3000.times.map do |i|
    _scratch = "scratch-#{i}-discarded-inner-allocation-padding"
    i + 1
  end
  puts r4.length          # 3000
  puts r4[0]              # 1
  puts r4[2999]           # 3000
end
t_gc_root_range_and_times_map

# === transpose_gc_root ===
def t_transpose_gc_root
  # `Array#transpose` codegen creates the result `sp_PtrArray` and
  # inner `sp_IntArray` columns without `SP_GC_ROOT`. With many
  # allocations across the loop, an `sp_gc_collect` triggered by
  # the next column's `sp_IntArray_new()` reclaims the unrooted
  # result PtrArray. The next `sp_PtrArray_push(result, col)`
  # dereferences a freed pointer.
  #
  # Trigger: a transpose of a large array-of-arrays. Optcarrot's
  # `(0..7).map { (0...0x10000).map {...} }.transpose` allocates
  # 8 outer + 65536 × 8 inner ints in close succession.
  
  big = (0..7).map { |a| (0..1023).map { |b| a * 1000 + b } }.transpose
  puts big.length        # 1024
  puts big[0].length     # 8
  puts big[100][3]       # 3*1000 + 100 = 3100
  puts big[1023][7]      # 7*1000 + 1023 = 8023
end
t_transpose_gc_root

# === utf8 ===
def t_utf8
  s = "あいうえお"
  
  puts s.length          # 5
  puts s.size            # 5
  puts s.bytesize        # 15
  
  puts s.chars.length    # 5
  s.chars.each { |c| puts c }
  
  puts s.reverse         # おえういあ
  
  puts s[0]              # あ
  puts s[1]              # い
  puts s[4]              # お
  puts s[-1]             # お
  puts s[1, 2]           # いう
  puts s[1..3]           # いうえ
  puts s.slice(2, 2)     # うえ
  
  puts "hello#{s}world".index(s)   # 5  (char index, not byte)
  puts s.index("う")               # 2
  puts "あいあい".rindex("い")    # 3
  
  puts "あいう".tr("い", "X")     # あXう
  puts "ababab".count("a")        # 3
  puts "あいあいあ".count("あ")   # 3
  puts "あいあいあ".delete("い")  # あああ
  puts "ああいいうう".squeeze     # あいう
  
  puts "あ".ljust(5, "*")          # あ****
  puts "あ".rjust(5, "*")          # ****あ
  puts "あ".center(5)              # _あ__   (with spaces)
  puts "あ".ljust(5).length        # 5   (chars, including padding)
  
  puts "あ".succ                  # ぃ
  puts "az".succ                  # ba
  
  puts s.include?("うえ")          # true
  puts s.start_with?("あい")       # true
  puts s.end_with?("えお")         # true
  
  # each_char with non-ASCII
  out = ""
  "日本語".each_char { |c| out = out + "[" + c + "]" }
  puts out                          # [日][本][語]
  
  # split on empty separator
  "あいう".split("").each { |c| puts c }   # あ\nい\nう
  
  # bytes is still byte-based
  puts "あ".bytes.length            # 3
end
t_utf8

