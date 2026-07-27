# Same lost laziness as `issue_3313_lazy_each_cons`, with the lazy chain bound
# to a local first -- assigning it drops the Lazy wrapper, so `each_cons` runs
# eagerly over an infinite range and the harness kills it at 60s.
s = (1..Float::INFINITY).lazy.select { |x| x > 3 }
p s.each_cons(2).first(2)
