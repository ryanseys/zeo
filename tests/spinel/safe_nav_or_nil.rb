class Inner005
  def opacity005; 0.5; end
  def name005; "hi"; end
  def size005; 3; end
end

class Outer005
  def initialize(c005); @c005 = c005; end
  def value005(override005); override005 || @c005&.opacity005; end
  def name005(override005); override005 || @c005&.name005; end
  def size005(override005); override005 || @c005&.size005; end
end

p Outer005.new(Inner005.new).value005(nil)
p Outer005.new(nil).value005(nil)
p Outer005.new(nil).value005(1.5)
p Outer005.new(Inner005.new).name005(nil)
p Outer005.new(nil).name005(nil)
p Outer005.new(Inner005.new).size005(nil)
p Outer005.new(nil).size005(nil)
