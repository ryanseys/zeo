# #515. Cross-variant Hash#merge between sym_str_hash and
# sym_poly_hash (sibling to #510) used to fail in both directions:
#
# - sym_str_hash.merge(sym_poly_hash): codegen routed to
#   sp_SymStrHash_merge and passed the poly arg as `b`, tripping
#   -Wincompatible-pointer-types and producing runtime garbage.
#
# - sym_poly_hash.merge(sym_str_hash): no helper at all,
#   "cannot resolve call to 'merge' on sym_poly_hash" warning.
#
# Fix: new sp_SymStrHash_to_sym_poly (codegen-emitted alongside
# the existing sym_str_hash runtime helpers) and
# sp_SymPolyHash_merge_str. Both directions return a fresh
# sym_poly_hash. Analyze's merge inference returns
# sym_poly_hash for the cross-variant pair.

def dir1_str_recv_poly_arg(t)
  poly = { method: :delete, id: 42, text: t }
  str  = { class: "btn", role: "button" }
  str.merge(poly)
end

def dir2_poly_recv_str_arg(t)
  poly = { method: :delete, id: 42, text: t }
  str  = { class: "btn", role: "button" }
  poly.merge(str)
end

# Both directions: receiver has 2 keys, arg has 3 keys, no
# overlap -> 5 entries in the merged hash.
puts dir1_str_recv_poly_arg("a").length
puts dir2_poly_recv_str_arg("a").length

# Overlap: arg's value wins (standard Hash#merge semantics).
def overlap_dir1
  base = { class: "btn", id: 1 }
  over = { id: 99, name: :foo }
  base.merge(over)[:id]
end
p overlap_dir1
