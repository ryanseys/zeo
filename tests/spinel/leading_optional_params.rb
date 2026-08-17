# Ruby funds the required parameters first and spends what is left on the
# optional ones, wherever they sit. Reading the argument list positionally put
# a lone argument in the leading optional and left the required one at its zero
# value, and the arity check rejected the call outright before that.
def a(x = 1, y)
  [x, y]
end
p a(8)
p a(7, 8)

def b(x = 1, y = 2, z)
  [x, y, z]
end
p b(9)
p b(8, 9)
p b(7, 8, 9)

# an optional in the middle
def c(p1, o = 2, r)
  [p1, o, r]
end
p c(1, 3)
p c(1, 2, 3)

class K
  def m(x = 1, y)
    [x, y]
  end
end
p K.new.m(8)
p K.new.m(7, 8)

e = ->(x = 1, y) { [x, y] }
p e.call(8)

# mixed with a keyword parameter
def f(x = 1, y, k: 5)
  [x, y, k]
end
p f(8)
p f(8, k: 9)

# the conventional shapes are untouched
def h(x, y = 2)
  [x, y]
end
p h(1)
p h(1, 3)

def i(x, y)
  [x, y]
end
p i(1, 2)

def j(x = 1, y = 2)
  [x, y]
end
p j
p j(8)
p j(8, 9)
