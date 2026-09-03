# `Hash.new` accepts a per-instance default that a missing key falls back to.

# A default VALUE: every missing key reads back as it (without being stored).
counts = Hash.new(0)
%w[a b a c a b].each { |w| counts[w] += 1 }
p counts                                  # {"a" => 3, "b" => 2, "c" => 1}
p counts["missing"]                       # 0
p counts.size                             # 3 -- the miss above stored nothing

# A default BLOCK is called with the hash and the missing key, and commonly
# memoizes by writing the computed value back into the hash.
squares = Hash.new { |h, k| h[k] = k * k }
p squares[4]                              # 16
p squares[9]                              # 81
p squares                                 # {4 => 16, 9 => 81}

# With neither, a missing key is nil, and `default` reports what was set.
plain = Hash.new
p plain[:nope]                            # nil
p Hash.new("fallback").default            # "fallback"
__END__
{"a" => 3, "b" => 2, "c" => 1}
0
3
16
81
{4 => 16, 9 => 81}
nil
"fallback"
