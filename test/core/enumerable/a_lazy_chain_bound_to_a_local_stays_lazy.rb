# A lazy select stored in a local still answers each_cons(2).first(2) over an
# infinite range.
# (spinel issue #3323)
s = (1..Float::INFINITY).lazy.select { |x| x > 3 }
p s.each_cons(2).first(2)
__END__
[[4, 5], [5, 6]]
