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
