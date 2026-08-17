# `equal?` with a boxed argument compares addresses: the same call reached
# with an object and with a symbol widens the parameter to poly, and the
# comparison used to fold to false for an object compared with itself (#3807).
Pad = Struct.new(:name)

class Plain
  def initialize(n)
    @n = n
  end
end

def same?(a, b)
  a.equal?(b)
end

pad = Pad.new('p1')
other = Pad.new('p1')
p same?(pad, pad)
p same?(pad, other)
p same?(pad, :sym)
p same?(pad, 7)
p same?(pad, nil)

def same_plain?(a, b)
  a.equal?(b)
end

x = Plain.new(1)
y = Plain.new(1)
p same_plain?(x, x)
p same_plain?(x, y)
p same_plain?(x, 'str')
