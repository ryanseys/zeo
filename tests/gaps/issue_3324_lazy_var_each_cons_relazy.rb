# Same lost laziness as `issue_3323_lazy_var_each_cons`, re-`lazy`-ed after the
# variable round-trip. The source here is an infinite prime sieve with an inner
# scan, so losing laziness is unbounded work; the harness kills it at 60s.
s = (2..Float::INFINITY).lazy.select { |n| (2...n).none? { |d| n % d == 0 } }
p s.each_cons(2).lazy.first(2)
s2 = (2..30).lazy.select { |n| (2...n).none? { |d| n % d == 0 } }
p s2.each_cons(2).lazy.first(2)
