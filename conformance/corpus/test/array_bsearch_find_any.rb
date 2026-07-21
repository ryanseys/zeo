# Array#bsearch / #bsearch_index find-ANY mode: an Integer-valued block follows
# the comparator protocol (0 found, negative searches left, positive right),
# distinct from the boolean find-minimum mode. Also: a float-array bsearch miss
# is nil (the result temp used to init as NULL, an invalid mrb_float).
a = [0, 4, 7, 10, 12]
p a.bsearch { |x| x <=> 7 }
p(a.bsearch { |x| x <=> 3 })
p a.bsearch_index { |x| x <=> 12 }
p(a.bsearch_index { |x| x <=> 11 })
p [1, 2, 3].bsearch { |x| 1 - x }
# find-minimum still works
p a.bsearch { |x| x >= 10 }
p a.bsearch_index { |x| x >= 5 }
b = [1.0, 2.5, 4.0]
p b.bsearch { |x| x >= 2.0 }
p(b.bsearch { |x| x >= 9.0 })
