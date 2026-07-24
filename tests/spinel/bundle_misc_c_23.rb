# Bundled tests:
#   - ptr_array_null_guards
#   - ptr_array_pop
#   - puts_mixed_ternary

# === ptr_array_null_guards ===
def t_ptr_array_null_guards
  # #566 (T.Yamada). The original report was a segfault from
  # downstream `Array#drop` / `take` running on a NULL pointer
  # emitted by then-unimplemented `each_cons` / `with_index` /
  # `max` (all warned-and-emitted-0 at the time). The NULL
  # guards in `sp_PtrArray_*` (commit 729b9cb) stopped the
  # segfault first; subsequently the missing Enumerator-chain
  # methods were implemented via chain fusion so the full
  # expression now produces CRuby's `3` instead of the
  # warned-fallback `0`.
  #
  # This test now covers two things together:
  # 1. The original NULL-safe guards on `sp_PtrArray_*` (a
  #    later regression that emits NULL where a PtrArray is
  #    expected must not crash here).
  # 2. The end-to-end Enumerator-chain `each_cons(n).
  #    with_index(off).map { ... }.drop.take.max[i]` lowering.
  
  q = [100, 90, 82, 70, 65]
  a = 2
  b = 4
  p q.each_cons(2).with_index(1).map { |(x, y), i| [x - y, i] }.drop(a - 1).take(b - a + 1).max[1]
end
t_ptr_array_null_guards

# === ptr_array_pop ===
def t_ptr_array_pop
  # #520. `Array#pop` on a nested integer array (Array<Array<Int>>,
  # spinel-side `int_array_ptr_array`) used to fall through to the
  # unresolved-call warning and emit 0 — the array stayed intact.
  # The same gap existed for any `<X>_ptr_array` element type; only
  # the IntArray / StrArray / FloatArray / SymArray flavors had a
  # direct `_pop` runtime helper.
  #
  # Fix: new sp_PtrArray_pop runtime helper (returns NULL on empty,
  # matching CRuby's nil) plus a dispatch arm in the
  # is_ptr_array_type recv_type branch.
  
  a = [[1, 2]]
  puts "before pop: #{a.inspect}"
  a.pop
  puts "after  pop: #{a.inspect}"
  a.push([3, 4])
  puts "after push: #{a.inspect}"
  
  # Seed-and-pop idiom: array typed as Array<String> via the seed,
  # then drained and used as an accumulator.
  b = ["seed"]
  b.pop
  b.push("real")
  b.push("data")
  puts b.inspect
  
  # Multiple pops drain the array.
  c = [[10], [20], [30]]
  c.pop
  c.pop
  puts c.length
end
t_ptr_array_pop

# === puts_mixed_ternary ===
def t_puts_mixed_ternary
  # Issue #640: `puts cond ? :sym : int` used to dispatch through
  # the wrong puts variant (`sp_sym_to_s(an_int)` printing garbage
  # instead of the int value). unify_return_type collapsed [int,
  # symbol] to "symbol" via the "int-as-default" heuristic, so the
  # else arm's int was passed through sym dispatch.
  #
  # Fix: compile_puts detects an IfNode arg whose then/else arms
  # have different *concrete* base types (neither nil/void) and
  # routes through sp_poly_puts so each arm's value is boxed to its
  # real tag.
  #
  # nil-arm cases (`if cond; X; end` without else) are left alone —
  # string and pointer types already handle nil through the existing
  # str_or_nil path.
  
  a = 2
  puts a%2>0 ? :odd : a    # else: prints "2"
  
  b = 3
  puts b%2>0 ? :odd : b    # then: prints "odd"
  
  # Float vs string
  c = 1.5
  puts c > 0 ? "positive" : c  # then: "positive"
  puts c > 5 ? "big" : c       # else: 1.5
end
t_puts_mixed_ternary

