class Obj9
  def initialize; @x = 7; end
end
a = Obj9.new
r = (a.instance_variable_set(:@y, 11) rescue $!.class)
p r
p a.instance_variable_get(:@y)
p a.instance_variable_get(:@x)

class C
  def initialize; @x = 1; end
  def go; instance_variable_set(:@x, 5); @x; end
  def get; instance_variable_get(:@x); end
  def has?; instance_variable_defined?(:@z); end
end
o = C.new
p o.go
p o.get
p o.has?
