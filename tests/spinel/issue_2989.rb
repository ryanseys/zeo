def m(order)
  order << :sym
end
a = []
m(a)
a << "str"
p a

def n(dst)
  dst << 1
end
b = []
b << "x"
n(b)
p b
