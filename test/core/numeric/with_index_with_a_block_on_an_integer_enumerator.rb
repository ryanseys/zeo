# The block form answers the source rather than the mapped values, for times,
# upto and downto.
# (spinel issue #3315)
p(4.times.with_index(1) { |x, i| x })
acc = []
p(3.upto(5).with_index(10) { |x, i| acc << [x, i] })
p acc
p(5.downto(3).each_with_index { |x, i| acc << [x, i] })
p acc
__END__
4
3
[[3, 10], [4, 11], [5, 12]]
5
[[3, 10], [4, 11], [5, 12], [5, 0], [4, 1], [3, 2]]
