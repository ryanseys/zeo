# Sibling case to str_poly_hash_dup_widen: the narrower-source variant
# is `sp_StrIntHash *` (int values). The fix wraps the `.dup` call
# with `sp_StrPolyHash_from_str_int_hash` so the slot's
# `sp_RbVal` reads round-trip the original int as a boxed int.
# Issue #614.

class Counter
  attr_reader :counts
  def initialize(h); @counts = h; end
end

c = Counter.new({ "a" => 1, "b" => 2 })
merged = c.counts.dup
merged["x"] = "label"   # str value forces analyzer-side widening
puts merged["a"]
puts merged["b"]
puts merged["x"]
