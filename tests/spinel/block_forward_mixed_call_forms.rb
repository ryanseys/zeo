def fwd(&b)
  [1, 2].map(&b)
end
tri = ->(x) { x * 3 }
p fwd(&tri)
p(fwd { |x| x * 3 })

def fwd2(&b)
  [1, 2].map(&b)
end
p(fwd2 { |x| x * 3 })
tri2 = ->(x) { x * 3 }
p fwd2(&tri2)

def fwd3(&b)
  [1, 2].map(&b)
end
str = ->(x) { "s#{x}" }
p fwd3(&str)
p(fwd3 { |x| x * 3 })
p(fwd3 { |x| x.to_s })

def sel(&b)
  [1, 2].select(&b)
end
odd = ->(x) { x.odd? }
p sel(&odd)
p(sel { |x| x > 1 })

def ea(&b)
  [1, 2].each(&b)
end
pr = ->(x) { p x }
ea(&pr)
ea { |x| p x * 10 }

def sm(&b)
  [1, 2, 3].sum(&b)
end
dbl = ->(x) { x * 2 }
p sm(&dbl)
p(sm { |x| x + 1 })

def mn(&b)
  [1, 2, 3].min_by(&b)
end
neg = ->(x) { -x }
p mn(&neg)
p(mn { |x| x })
p(mn { |x| x.to_s })

def mx(&b)
  [1, 2, 3].max_by(&b)
end
p mx(&neg)
p(mx { |x| x })

def gb(&b)
  [1, 2, 3].group_by(&b)
end
p gb(&dbl)
p(gb { |x| x + 1 })

def ct(&b)
  [1, 2, 3].count(&b)
end
big = ->(x) { x > 1 }
p ct(&big)
p(ct { |x| x < 3 })

def sb(&b)
  [3, 1, 2].sort_by(&b)
end
p sb(&neg)
p(sb { |x| x })

def rg(&b)
  (1..3).map(&b)
end
p rg(&dbl)
p(rg { |x| x + 1 })

def fs(&b)
  [1, 2, 3].sum(&b)
end
half = ->(x) { x * 0.5 }
p fs(&half)
p(fs { |x| x + 1 })
