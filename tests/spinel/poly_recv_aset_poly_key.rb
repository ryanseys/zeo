# #464: `merged[k] = v` where merged is poly-recv AND k is also
# poly. Pre-fix, the poly recv `[]=` dispatcher emitted only
# Array setter arms which expect mrb_int index, so passing
# sp_RbVal as the index argument failed C compile.
#
# Fix: when idx is poly, dispatch by both recv cls_id AND
# key tag at runtime.

def get_poly(h, key)
  h[key]
end

def write_back(merged, k, v)
  merged[k] = v
end

def run(outer)
  merged = get_poly(outer, "m")
  # k and v as poly via a helper that lets analysis lose their
  # concrete type. wrap_key boxes a string into sp_RbVal via
  # the poly_hash leaf.
  pairs = get_poly(outer, "pairs")
  k = get_poly(pairs, "k")
  v = get_poly(pairs, "v")
  write_back(merged, k, v)
  merged["b"]
end

outer = { "m" => { "a" => 1 }, "pairs" => { "k" => "b", "v" => 99 } }
puts run(outer)
