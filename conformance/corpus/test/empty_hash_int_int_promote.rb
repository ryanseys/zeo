# An empty `{}` hash that then receives an integer-literal key must
# promote from the str_int_hash empty-default to int_int_hash.
# Otherwise the integer key is passed to sp_StrIntHash_set/_get as a
# `const char *` and dereferenced, segfaulting. Gated on Integer
# *literal* keys (an unresolved/variable key whose type is the "int"
# first-pass fallback must NOT promote, or a later-resolved String key
# would be stored into an int_int_hash).

h = {}
h[1] = 10
h[2] = 20
puts h[1].to_s
puts h[2].to_s
puts h.size.to_s

# present-key arithmetic and has_key? guard
puts (h[1] + h[2]).to_s
puts h.has_key?(5).to_s
puts h.has_key?(1).to_s

# overwrite an existing key
h[1] = 11
puts h[1].to_s

# method scope
def build_counts
  m = {}
  m[100] = 1
  m[200] = 2
  m[100] + m[200]
end
puts build_counts.to_s
