def ins(arr, v) = arr.insert(0, v)
s = []
[5, 2, 8].each { |n| ins(s, n) }
p s.bsearch { |x| x >= 4 }
p [1, 2, 5, 8].bsearch { |x| x >= 4 }
t = []
t << 2
t << 4
t << 9
p t.bsearch { |x| x >= 4 }
