def m(a)
  yield a
end
p(m([1, 2]) { |*a, **k| [a, k] })
p(m([1, 2]) { |a, b, **k| [a, b, k] })
def m2(x, y)
  yield x, y
end
p(m2(1, 2) { |a, b, **k| [a, b, k] })
def m3(a)
  yield a, x: 9, y: 8
end
p(m3(1) { |v, x:, **k| [v, x, k] })
