# A slot typed as a class that HAS subclasses is only its STATIC type: an
# inherited method storing `self` types the slot as the class that DEFINED the
# method, while the object can be any subclass. The read then refused a method
# the runtime object answers perfectly well (#4023):
#
#   Column.new.attach(n); n.link.size
#   # undefined method 'size' for an instance of Node
#
# `.class` on the same read has always answered Column -- the boxed value
# carries the right id -- so only the static refusal was wrong.
class Node
  attr_accessor :link

  def initialize
    @link = nil
  end

  def attach(other)
    other.link = self
    self
  end
end

class Column < Node
  attr_accessor :size

  def initialize
    super()
    @size = 9
  end
end

n = Node.new
c = Column.new
c.attach(n)
p n.link.class
p n.link.size

# the base's own members keep working through the same slot
m = Node.new
Node.new.attach(m)
p m.link.class
p m.link.link.inspect

# and a subclass stored directly, not through the inherited method
k = Node.new
k.link = Column.new
p k.link.size
