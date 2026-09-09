# A reduce seeded with a Symbol whose block value comes from a two-level
# constant hash.
# (spinel issue #2903)
T = { a: { "0" => :a, "1" => :b }, b: { "0" => :a, "1" => :b } }
p "01".chars.reduce(:a) { |s, c| T[s][c] }
p "10".chars.reduce(:a) { |s, c| T[s][c] }
__END__
:b
:a
