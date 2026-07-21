# Splat destructure with a poly_array RHS — Gemini concern on PR #116:
# the runtime-slice path of compile_multi_write_splat had no case for
# poly_array, so it fell through to sp_IntArray_get / sp_IntArray_slice
# and the C compile rejected the type mismatch. With the fix the path
# uses sp_PolyArray_get / sp_PolyArray_slice for poly_array RHS.

def heterogeneous
  ["alpha", 1, "beta", 2.5, "gamma"]
end

# Trailing splat — splat target is a sp_PolyArray * holding the tail.
a, b, *rest = heterogeneous
puts a              # alpha
puts rest.length    # 3

# Leading splat — splat target holds the head, then two scalar tails.
*head, y, z = heterogeneous
puts head.length    # 3
puts z              # gamma
puts y              # 2.5  (Ruby) — verifies float scalar from poly RHS

# Middle splat — most exercise of the slice + bracketing helpers.
p, *mid, q = heterogeneous
puts mid.length     # 3
puts p              # alpha
puts q              # gamma
