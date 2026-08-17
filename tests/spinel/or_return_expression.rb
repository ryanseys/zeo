def fetch006(h006, k006)
  v006 = h006[k006] or return
  v006
end
p fetch006({ a: 1 }, :a)
p fetch006({ a: 1 }, :b)

def g(x)
  y = x or return "none"
  "got #{y}"
end
p g(nil)
p g(5)

def h(x)
  y = x || return
  y * 2
end
p h(3)
p h(nil)

def sum_positive(a)
  t = 0
  a.each do |v|
    v > 0 or next
    t += v
  end
  t
end
p sum_positive([1, -2, 3])

def first_neg(a)
  found = nil
  a.each do |v|
    v < 0 and break
    found = v
  end
  found
end
p first_neg([1, 2, -3, 4])
