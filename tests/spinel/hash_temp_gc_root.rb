# #689: Hash temporaries created from `{}` literals must be GC-rooted
# before subsequent allocations, otherwise GC may reclaim them mid-
# expression and the consuming call reads NULL+offset.
#
# Simulates the failing pattern: two hash literals constructed in
# sequence, the first consumed by a helper that allocates more.

def two_hashes(a, b)
  a.length + b.length
end

# Drive GC by allocating between binding and consumption.
i = 0
while i < 5000
  h1 = {"x" => 1, "y" => 2}
  h2 = {"a" => 3, "b" => 4}
  # Allocate strings to force GC pressure between the two hash
  # literals (the same pattern that #689 hits with stylesheet_link_tag).
  filler = "filler-" + i.to_s + "-" + i.to_s
  if two_hashes(h1, h2) != 4
    puts "broken at #{i}"
    exit 1
  end
  i += 1
end
puts "ok"
