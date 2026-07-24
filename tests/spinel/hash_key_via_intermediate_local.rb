# Sibling to #443. `h[pp[1..]] = ap` where pp/ap are intermediate
# locals (themselves split-result elements) had the hash inferred
# as str_int_hash instead of str_str_hash. Root cause: the
# refine_method_body_locals 2-pass merge resolved pp's type
# (str_array element → string), but the merge happened AFTER the
# pass-2 scan_locals run. Inside that pass-2 scan, infer_type(pp)
# read pp's pass-1 type (still "int" because pa wasn't declared
# during pass 1) from global scope, so `h[pp[1..]] = ap` promoted
# the empty hash to int_str_hash and the merge dropped the
# correction.
#
# Fix: add a pass-3 scan_locals to refine_method_body_locals,
# run after the merge has corrected intermediate-local types in
# scope. The third pass re-resolves nested-CallNode key/value
# expressions (pp[1..], pp.upcase, etc.) against the merged
# scope, picking up the right hash variant.

def build(a, b)
  pa = a.split("/")
  pb = b.split("/")
  h = {}
  i = 0
  while i < pa.length
    pp = pa[i]
    ap = pb[i]
    h[pp + "_k"] = ap
    i += 1
  end
  h.length
end

puts build("a/b/c", "x/y/z")
