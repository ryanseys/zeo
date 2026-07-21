# `Hash#each_value { |v| ... }` for poly_poly_hash. The previous
# dispatch only knew about `each` / `each_pair`; `each_value` fell
# through to the unresolved-call warning and silently dropped the
# block body.
#
# Surfaced via optcarrot's PPU init
# `entries.each_value {|a| a.uniq! {|entry| entry.object_id } }`
# where `entries` is a heterogeneous-key heterogeneous-value hash.

h = {}
h[[1, 2]] = "ab"
h[[3, 4]] = "cd"
h["s"] = 99
h[5] = [10, 20]

# Each value passed to the block in insertion order; key ignored.
h.each_value do |v|
  puts v.to_s
end
puts "---"

# Mutating the block's local doesn't affect the hash.
h.each_value do |v|
  v = "ignored"
  puts v
end
puts "---"

# No block param: still iterates (counts side-effecting expr).
i = 0
h.each_value do
  i = i + 1
end
puts i
