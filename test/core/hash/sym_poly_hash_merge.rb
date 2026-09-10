# `Hash#merge` on a Symbol-keyed hash whose values are of mixed type
# (Integer, String, Boolean and nil together). The merge is non-mutating, so
# the receiver keeps its own pairs and the result carries both sets.
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

# 3. Symbol-key `[]` lookup on a local the compiler cannot give one type:
#    the `is_a?(String)` branch reassigns it, so the reads after the merge
#    have to go through the general path.
def lookup(opt)
  # A mixed-value override hash, so the branch and the call site disagree
  # about the value type.
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
__END__
99
TWO
true
16
1
7
two
16
100
B
99
