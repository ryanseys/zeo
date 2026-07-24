# `self.instance_variable_set` mutates the receiver, so the class cannot keep
# the by-value representation (the write would land on a copy and be lost).
class C
  def initialize; @z = 0; end
  def set; self.instance_variable_set(:@z, 5); end
  def get; @z; end
end
c = C.new
c.set
p c.get
p c.instance_variable_get(:@z)

class D
  def initialize; @a = 1; end
  def bump; self.instance_variable_set(:@a, @a + 10); end
  def a; @a; end
end
d = D.new
d.bump
d.bump
p d.a
