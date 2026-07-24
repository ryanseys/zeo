# `Hash#merge` on a Symbol-keyed hash with heterogeneous values
# (Symbol => {Integer | String | Boolean | nil}) -- spinel
# represents this as `sym_poly_hash`. Before this change spinel had
# no dispatcher for non-mutating `merge` on sym_poly_hash, so the
# call lowered to an empty hash and every subsequent lookup returned
# nil.
#
# The poly-receiver `[]` arm covers the case where a local's static
# type was widened to poly (e.g. by an `is_a?` branch) even though
# the runtime value is a sym_poly_hash. `opt[:k]` then dispatches
# via the emit_poly_builtin_dispatch helper rather than the typed
# sym_poly_hash branch above.

DEFAULTS = { a: 1, b: "two", c: true, d: nil, e: 16 }

# 1. Non-mutating merge: result has overrides from b, other keys
#    preserved from a.
m = DEFAULTS.merge({ a: 99, b: "TWO" })
puts m[:a]              # 99 (overridden)
puts m[:b]              # TWO
puts m[:c]              # true (preserved)
puts m[:e]              # 16

# 2. Original receiver is unchanged.
puts DEFAULTS[:a]       # 1

# 3. Symbol-key `[]` lookup on a poly-typed local. The is_a?(String)
#    branch widens `opt`s static type to poly; the merge-result is
#    boxed back into the same slot. spinel must dispatch the
#    subsequent `opt[:k]` reads via the poly builtin path.
def lookup(opt)
  # Mixed-value override hash so spinel infers sym_poly_hash for the
  # `is_a?(String)` boxed branch (and the explicit-Hash call site too).
  opt = { a: 7, c: false } if opt.is_a?(String)
  merged = DEFAULTS.merge(opt)
  return merged[:a], merged[:b], merged[:e]
end

a, b, e = lookup("string-input")
puts a                  # 7  (from the boxed hash inside the if branch)
puts b                  # two
puts e                  # 16

a, b, e = lookup({ a: 100, b: "B", e: 99 })
puts a                  # 100
puts b                  # B
puts e                  # 99
