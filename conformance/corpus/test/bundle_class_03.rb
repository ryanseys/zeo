# Bundled tests:
#   - chained_or_assign_collection_poly_array
#   - chained_or_assign_collection_str_keys

# === chained_or_assign_collection_poly_array ===
# `(arr[idx] ||= []) << v` — chained `||=` over a poly_array
# element. Pre-fix the IndexOrWriteNode expression-form returned
# the int default for non-hash receivers, so the chain receiver
# collapsed to literal `0` and `<<` failed T_chained_or_assign_collection_poly_array_C compile.
# Spinel now lowers the get-then-set into an sp_RbVal temp that
# the chain reads — same shape as the *_poly_hash arms but with
# in-bounds + auto-grow handling for the indexed access.

class T_chained_or_assign_collection_poly_array_C
  def initialize
    @a = Array.new(3) { [] }    # promotes to poly_array of empty poly_arrays
  end
  def push(idx, v)
    (@a[idx] ||= []) << v
  end
  def count(i); @a[i].length; end
end

c = T_chained_or_assign_collection_poly_array_C.new
c.push(0, 10)
c.push(0, 20)
c.push(1, 100)
puts c.count(0)   # 2
puts c.count(1)   # 1
puts c.count(2)   # 0

# === chained_or_assign_collection_str_keys ===
# Same chained-`||=` lowering, but for `str_poly_hash` instead of
# `sym_poly_hash`. Exercises the `const char *` key path (vs the
# `sp_sym` integer key path) of the typed-poly-hash branch added
# alongside the sym_poly_hash arm.

class T_chained_or_assign_collection_str_keys_C
  def initialize
    @h = {}
    @h["init"] = []      # forces str_poly_hash
  end
  def add(k, v)
    (@h[k] ||= []) << v
  end
  def count(k); @h[k].length; end
end

c = T_chained_or_assign_collection_str_keys_C.new
c.add("init", 7)         # appends to existing
c.add("foo", 1)
c.add("foo", 2)
c.add("bar", 100)
puts c.count("init")     # 1
puts c.count("foo")      # 2
puts c.count("bar")      # 1

