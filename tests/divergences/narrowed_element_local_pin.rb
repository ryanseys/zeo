# Same cause as `sort_with_comparator.rb`: `Array#sort!` is STABLE in zeo
# (Rust's `sort_by`) and UNSTABLE in ruby (`ruby_qsort`). The comparator below
# orders by a pair of ranks, so ties are common and the two implementations
# break them differently. Only the FIRST line diverges; the second is the
# property the file was imported for and it agrees.
#
# That second property is why the file is worth keeping rather than deleting as
# a duplicate: a local bound from a container read (`pair_a = pairs[a]`, where
# `pairs` is a table of int arrays) is narrowed to the element type, and that
# decision once shared a pin field with the pointer-array narrowing. The array
# pass put every pinned slot back on the poly array on the way in, the element
# pass narrowed it again, and the two traded the slot every round -- so the
# fixpoint never converged and every compile ran its whole round budget, keeping
# whatever types the cap happened to catch. (The LangArena program of #3781
# compiled in 15s and lost an unrelated method's element type that way; it now
# settles in 1.3s.)
#
# Imported from the spinel corpus at c55d9bdb; see tests/spinel/UPSTREAM.md.
#
# --- ruby 4.0.6 answers ---
# [[2, 5, 4, 1, 7, 3, 6, 0], [3, 1, 0, 3, 1, 0, 3, 2]]
# [[0, 2, 1], [0, 2, 1]]

# A DECIDED DIVERGENCE on its FIRST line only: `Array#sort!` is STABLE in zeo
# (Rust's `sort_by`) and UNSTABLE in ruby (`ruby_qsort`), and this comparator
# orders by a pair of ranks, so ties are common. The golden records ZEO's
# output; the `.divergence` sidecar carries the reason and ruby's answer.
#
# The SECOND line is what the file was imported for, and it agrees:
#
# A local bound from a container read (`pair_a = pairs[a]`, where `pairs` is a
# table of int arrays) is narrowed to the element type. That decision and the
# pointer-array narrowing shared one pin field: the array pass put every pinned
# slot back on the poly array on the way in, the element pass narrowed it again,
# and the two traded the slot every round -- so the fixpoint never converged and
# every compile ran its whole round budget, leaving whatever types the cap
# happened to catch. (The LangArena program of #3781 compiled in 15s and lost
# an unrelated method's element type that way; it now settles in 1.3s.)

def sort_pairs(n)
  rank = Array.new(n) { |i| (n - i) % 3 }
  pairs = Array.new(n) { |i| [rank[i], rank[(i + 1) % n]] }
  sa = Array.new(n) { |i| i }

  sa.sort! do |a, b|
    pair_a = pairs[a]
    pair_b = pairs[b]
    if pair_a[0] != pair_b[0]
      pair_a[0] <=> pair_b[0]
    else
      pair_a[1] <=> pair_b[1]
    end
  end

  ranks = Array.new(n, 0)
  i = 1
  while i < n
    prev_pair = pairs[sa[i - 1]]
    curr_pair = pairs[sa[i]]
    ranks[sa[i]] = ranks[sa[i - 1]] + (prev_pair != curr_pair ? 1 : 0)
    i += 1
  end
  [sa, ranks]
end

p sort_pairs(8)
p sort_pairs(3)
