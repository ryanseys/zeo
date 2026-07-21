# Bundled tests:
#   - io_call_in_proc_body
#   - local_hash_cross_type_widen
#   - map_block_string_new

# === io_call_in_proc_body ===
def t_io_call_in_proc_body
  # `puts` / `print` / `printf` reached in expression context — the
  # last statement of a `proc { ... }` body. The expression-context
  # dispatcher previously had no path for these no-recv IO methods,
  # so the call fell through to warn_unresolved_call and the IO
  # side-effect was silently dropped. Fix: bridge through
  # compile_io_call_stmt and pass "0" up as the C expression value.
  #
  # To exercise all three IO methods concisely, the test iterates an
  # array of procs and `.call`s each. This relies on two adjacent
  # fixes also in this PR:
  #
  # 1. Heterogeneous array literals of procs (`[proc, proc, proc]`)
  #    are inferred as `poly_array` (boxed via sp_box_proc) rather
  #    than falling through to `int_array`. Without this, the array
  #    construction emitted `sp_IntArray_push` with a `sp_Proc *`
  #    argument and the C compiler rejected the int-from-pointer
  #    conversion.
  #
  # 2. Poly `.call` dispatch in `compile_poly_method_call` — when a
  #    block param iterates a poly array carrying procs, the recv's
  #    static type is `poly`. Unbox via `(sp_Proc *)recv.v.p` and
  #    invoke `sp_proc_call`. Without this, `p.call` inside the
  #    block fell through to warn_unresolved_call on a poly recv.
  #
  # Symbol#to_proc (`&:call`) — the more idiomatic Ruby form for this
  # iteration — is still TODO in spinel (`find_block_arg` returns -1
  # for the SymbolNode shape). The explicit-block form `{ |p| p.call }`
  # is the closest equivalent that works today; the &:sym lowering is
  # a separate concern.
  
  [
    proc { puts "a" },
    proc { print "b\n" },
    proc { printf("c=%d\n", 3) }
  ].each { |p| p.call }
end
t_io_call_in_proc_body

# === local_hash_cross_type_widen ===
def t_local_hash_cross_type_widen
  # Cross-type `[]=` write on a non-empty hash literal should widen the
  # LV's hash type so the store accepts the new value via boxing.
  # Before this fix, `m = {"a" => "1"}; m["k"] = 42` lowered to
  # `sp_StrStrHash_set(m, "k", 42LL)` -- gcc rejected the mrb_int passed
  # where a `const char *` was expected.
  #
  # Fix: scan_locals's CallNode `[]=` arm widens the LV from a narrow
  # variant (str_str_hash / sym_int_hash / etc.) to its corresponding
  # `*_poly_hash` when the new write's key or value type doesn't fit
  # the current variant. The LV-write emit then builds the literal
  # init as the widened poly variant directly (boxing each value).
  # Issue #589.
  
  # str_str_hash → str_poly_hash via int value
  m = {"a" => "1"}
  m["k"] = 42
  puts m.length          # 2
  puts m["a"]            # "1"
  puts m["k"]            # 42
  
  # str_int_hash → str_poly_hash via string value
  n = {"x" => 10, "y" => 20}
  n["z"] = "thirty"
  puts n.length          # 3
  puts n["x"]            # 10
  puts n["z"]            # "thirty"
  
  # sym_int_hash → sym_poly_hash via string value
  o = {a: 1, b: 2}
  o[:c] = "three"
  puts o.length          # 3
  puts o[:a]             # 1
  puts o[:c]             # "three"
  
  # sym_str_hash unchanged when same value-type written
  p = {x: "one", y: "two"}
  p[:z] = "three"
  puts p.length          # 3
  puts p[:y]             # two
  
  # str_str_hash unchanged when same value-type written -- the widening
  # arm only fires on a real key/value mismatch, so consistent writes
  # stay on the narrow variant.
  q = {"a" => "1"}
  q["b"] = "2"
  puts q.length          # 2
  puts q["b"]            # "2"
end
t_local_hash_cross_type_widen

# === map_block_string_new ===
def t_map_block_string_new
  # #522. Sibling to #519. The literal-array path `[String.new]` was
  # fixed in #519 to infer as `mutable_str_ptr_array` (a sp_PtrArray
  # of sp_String*). The `.map { String.new }` path didn't share the
  # inference — block-return type `mutable_str` was missing from the
  # map-result type cascade, so the accumulator stayed at the
  # default int_array shape and the push failed C compilation with
  # `sp_IntArray_push(arr, sp_String_new(""))`.
  #
  # Fix: add a `mutable_str` arm in both the analyzer
  # (infer_call_type's map branch -> mutable_str_ptr_array) and the
  # codegen (int_array recv branch -> sp_PtrArray accumulator with
  # `(void *)val` push).
  
  xs = [1, 2, 3]
  
  # Block returns String.new directly.
  result = xs.map { |x| String.new }
  puts result.length
  
  # Block returns a local widened to mutable_str.
  result2 = xs.map { |x| s = String.new; s << "v"; s }
  puts result2.length
end
t_map_block_string_new

