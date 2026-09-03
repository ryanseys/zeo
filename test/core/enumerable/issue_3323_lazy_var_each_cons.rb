# The same laziness as `issue_3313_lazy_each_cons`, with the lazy chain bound
# to a local first -- the round trip through a variable keeps the Lazy.
s = (1..Float::INFINITY).lazy.select { |x| x > 3 }
p s.each_cons(2).first(2)
__END__
[[4, 5], [5, 6]]
