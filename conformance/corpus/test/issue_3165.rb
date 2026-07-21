def ins(arr, v) = arr.insert(0, v)
sorted = []
ins(sorted, 5)
p sorted.bsearch_index { |x| x >= 4 }

def build(a, v); a.push(v); a; end
xs = []
build(xs, 1); build(xs, 3); build(xs, 5); build(xs, 7)
p xs.bsearch_index { |x| x >= 4 }
p xs.bsearch_index { |x| x >= 100 }
p xs.bsearch_index { |x| 5 <=> x }

a = [1, 3, 5, 7, 9]
p a.bsearch_index { |x| x >= 6 }
