# An attribute that holds a base class AND its subclass reads as a boxed value.
# A local assigned from one had settled on the object type, and kept the
# pointer while the read handed back an sp_RbVal -- every such assignment was a
# build error (#4023):
#
#   error: incompatible types when assigning to type 'sp_Node *' from 'sp_RbVal'
class Node
  attr_accessor :right, :column

  def initialize
    @right = self
    @column = nil
  end
end

class Column < Node
  attr_accessor :size

  def initialize
    super()
    @size = 0
    @column = self
  end
end

col = Column.new
n = Node.new
n.right = col
n.column = col
col.right = n

# the ring holds both classes, so each read is boxed
j = n.right
p j.class
k = j.right
p k.class
m = n.column
p m.class
p m.size

# walking it keeps working
cur = col
2.times do
  cur = cur.right
end
p cur.class
