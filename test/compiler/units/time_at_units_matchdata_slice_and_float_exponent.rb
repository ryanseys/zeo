# MatchData indexes like an array `[full, g1, g2, g3]`, so `md[1, 2]`
# is `[g1, g2]` and `md[1..]` is `[g1, g2, g3]` (verified against
# ruby 4.0.6 -- the previous expectation dropped the first capture).

p Time.at(0, 500, :millisecond).to_f
p Time.at(0, 500, :nanosecond).to_f
md = "2024-01-31".match(/(\d+)-(\d+)-(\d+)/)
p md[1, 2]
p md[1..]
p(md == "2024-01-31".match(/(\d+)-(\d+)-(\d+)/))
p 5.0e-7
p 1.0e20
__END__
0.5
5.0e-07
["2024", "01"]
["2024", "01", "31"]
true
5.0e-07
1.0e+20
