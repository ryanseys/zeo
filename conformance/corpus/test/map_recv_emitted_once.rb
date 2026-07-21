# #540. `n, m = gets.split.map &:to_i` emitted `gets` twice in
# the generated C: once via compile_call_expr's pre-compute of
# the call recv (`rc = compile_expr_gc_rooted(recv)`) and once
# inside compile_map_expr's own `compile_expr(@nd_receiver[nid])`.
# For pure-function recvs the duplicate is wasted work, but for
# `gets` (and any other side-effecting call) it consumed two
# lines of stdin per iteration -- the multi-assign got the
# second line's values, then the next loop iteration consumed
# two more lines, and the loop's `break if n == 0` check never
# saw the intended `0 0` row.
#
# Fix: compile_call_expr threads its pre-computed `rc` through
# compile_enumerable_expr to compile_map_expr (and a new
# `precomputed_rc` param defaults to "" on the
# compile_body_return tail path that still wants to compile the
# recv itself). compile_map_expr uses the passed temp directly
# but still SP_GC_ROOTs it -- compile_expr_gc_rooted skips the
# root for non-CallNode recvs (ArrayNode literals like
# `[10,20,30,40].map { ... }` reach us unrooted), so the
# transient-root emit has to happen here.

# Counter-style verifies side-effect-once. Each call to `next!`
# bumps the counter; the test asserts the counter only moves by
# the expected amount across the multi-assign.
$counter = 0
def next!
  $counter += 1
  $counter.to_s + " " + ($counter + 100).to_s
end

# Multi-assign over a side-effecting recv. `next!` returns the
# string; the chain splits/maps it.
a, b = next!.split.map { |x| x.to_i }
puts a
puts b
puts $counter  # was 2 pre-fix (called twice); 1 post-fix.

# Nested map with array-literal recv (the GC-rooting concern --
# the recv temp is the literal, no other root holds it).
TBL = [10, 20, 30, 40].map { |a| (0...8).map { |j| a + j } }
puts TBL.length         # 4
puts TBL[0].length      # 8
puts TBL[0][0]          # 10
puts TBL[3][7]          # 47
