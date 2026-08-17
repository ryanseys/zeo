def outer(&b)
  inner(&b)
end
def inner(&b)
  [1, 2].map(&b)
end
tri = ->(x) { x * 3 }
p outer(&tri)
p(outer { |x| x * 4 })

def a3(&b) = a2(&b)
def a2(&b) = a1(&b)
def a1(&b) = [1, 2].map(&b)
p a3(&tri)
p(a3 { |x| x + 1 })

def s_outer(&b) = s_inner(&b)
def s_inner(&b) = ["a", "bb"].map(&b)
len = ->(s) { s.length }
p s_outer(&len)
p(s_outer { |s| s.upcase })

def y_outer(&b) = y_inner(&b)
def y_inner(&b) = y_leaf(&b)
def y_leaf
  yield 5
end
p y_outer(&tri)
p(y_outer { |x| x + 100 })

def twice(&b)
  [10].map(&b) + inner(&b)
end
p twice(&tri)
