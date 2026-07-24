# A yield-using method called with a literal block returns the yielded
# value, typed by the block's return type (not the int default). Covers
# instance + top-level methods, expression / assignment contexts, and
# value vs string vs concat-with-yield bodies.

# int yield, instance method, expression position
class W
  def s
    yield
  end
end
puts W.new.s { 7 }

# string yield, instance method (bare yield is the return value)
puts W.new.s { "str" }

# string yield used inside the method body (concat with yield)
class G
  def wrap
    "[" + yield + "]"
  end
end
puts G.new.wrap { "x" }

# top-level method, string yield
def t
  yield
end
puts t { "hi" }

# string yield stored in a local: the pointer round-trips through the
# int slot and reads back as a string (no -Wint-conversion).
sv = W.new.s { "str" }
puts sv

# assignment context with an int yield (no pointer round-trip)
n = W.new.s { 11 }
puts n

# block with a parameter, int arithmetic
class A
  def give
    yield 5
  end
end
puts A.new.give { |x| x + 1 }

# yield value flowing through interpolation
def deco
  "<#{yield}>"
end
puts deco { "body" }
