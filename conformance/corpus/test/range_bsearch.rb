# Integer Range#bsearch in find-minimum mode: the block returns truthy
# for the upper half; bsearch returns the smallest member where it does,
# or nil when no member qualifies.
p (1..100).bsearch { |x| x >= 42 }
p (1..100).bsearch { |x| x >= 1 }
p (1..100).bsearch { |x| x >= 100 }
p (1..100).bsearch { |x| x >= 200 }
p (0...10).bsearch { |x| x >= 7 }
p (0...10).bsearch { |x| x >= 9 }
p (0...10).bsearch { |x| x >= 10 }
p (5..5).bsearch { |x| x >= 5 }
