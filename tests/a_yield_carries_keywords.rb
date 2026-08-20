def m1
  yield 1, a: 2, b: 3
end
m1 { |x, **kw| p [x, kw] }
m1 { |x, kw| p [x, kw] }

def m2(h)
  yield 1, **h
end
m2({}) { |*a| p a }
m2({z: 9}) { |*a| p a }

def m3(h)
  yield(**h)
end
m3({}) { |*a| p a }
m3({q: 1}) { |*a| p a }

def m4(arr, h)
  yield(*arr, **h)
end
m4([1,2], {}) { |*a| p a }
m4([1,2], {k: 3}) { |*a| p a }

def m5
  yield a: 1
end
m5 { |kw| p kw }
m5 { |a:| p a }
