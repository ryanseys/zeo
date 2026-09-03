# A Numeric block result selects find-any mode: 0 is a hit, negative
# searches the lower half, positive the upper; a non-hit answers nil. The
# boolean find-minimum mode still works alongside it.

a = [0, 4, 7, 10, 12]
p a.bsearch { |x| 7 <=> x }
p a.bsearch { |x| 10 <=> x }
p a.bsearch_index { |x| 12 <=> x }
p(a.bsearch { |x| 3 <=> x })
p a.bsearch { |x| x >= 10 }
p [1, 2, 3].bsearch { |x| 1 - x }
p((1..100).bsearch { |x| x >= 8 })
__END__
7
10
4
nil
10
1
8
