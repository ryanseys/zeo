# `[].sample` with nothing to draw, `union` with no arguments, and two more
# shapes with no operand to work on.

# Empty Array#sample returns nil, matching CRuby (was the zero value; #2322).
puts [].sample.inspect

# No-args Array#union reduces to dedup.
puts [1, 2, 3, 2].union.inspect

# Mixed-key hash literal builds via poly_poly_hash.
h = { 0 => false, a: 1 }
puts h.size
puts h[:a]
__END__
nil
[1, 2, 3]
2
1
