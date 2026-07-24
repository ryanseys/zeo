# `h = {}; h[parts[0]] = v` where `parts` comes from a `split` result
# (or any expression that returns str_array). Two pre-fix shapes
# combined:
#
# 1. kt="int" + vt="int" — pass-0 promote_empty_hash_for widened to
#    poly_poly_hash via the catch-all. Now defers (returns "") so the
#    second pass with the key resolved as string produces
#    str_int_hash.
# 2. kt="int" + vt="string" — pass-0 promoted to int_str_hash, and
#    the refine_locals merge had no rule to upgrade int_str_hash to
#    str_str_hash when pass 1 resolved the key as string. Added
#    int_str_hash → str_str_hash / sym_str_hash widening rules.
#
# Sibling check: a literal-keyed `h = {}; h[200] = "OK"` (legitimate
# int-keyed-string-valued) keeps int_str_hash.

def build_str_keyed_int_val(s)
  parts = s.split("/")
  h = {}
  h[parts[0]] = 1
  h
end

def build_str_keyed_str_val(s)
  parts = s.split("/")
  h = {}
  h[parts[0]] = "v"
  h
end

def build_int_keyed
  h = {}
  h[200] = "OK"
  h[404] = "NotFound"
  h
end

h1 = build_str_keyed_int_val("a/b")
puts h1["a"]            # 1

h1b = build_str_keyed_str_val("a/b")
puts h1b["a"]           # v

h2 = build_int_keyed
puts h2[200]            # OK
puts h2[404]            # NotFound
