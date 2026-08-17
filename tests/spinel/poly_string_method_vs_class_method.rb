# A class method of the same name is reached only through a Class-valued
# receiver: it must not capture a String instance call in a poly union.
class Foo
  def self.downcase(s)
    s + "!"
  end

  def self.upcase(s, n)
    s * n
  end
end

def pick(n)
  if n > 0
    "HeLLo"
  else
    Foo
  end
end

k = pick(1)
p k.downcase
p k.upcase
p k.reverse
p k.chars.first(2)

# the class method still answers through the class itself
p Foo.downcase("x")
p Foo.upcase("y", 3)

# and an instance member of the name still wins over the String method
Row = Struct.new(:upcase)
def pick2(n)
  n > 0 ? Row.new(7) : "text"
end
p pick2(1).upcase
p pick2(0).upcase
