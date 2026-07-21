class Box
  def initialize; @v = 1; @s = "hi"; end
  def drop; remove_instance_variable(:@v); end
  def drops; remove_instance_variable(:@s); end
end
b = Box.new
p b.drop
p b.drops
class C
  def initialize; @x = 5; end
end
c = C.new
r = begin; c.remove_instance_variable(:@nope); rescue => e; e.class; end
p r
