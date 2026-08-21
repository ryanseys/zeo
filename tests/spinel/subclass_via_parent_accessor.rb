# A parent holds an accessor pointing at another object of the same family, and
# a parent method writes `self` into it. Reading it back and calling a method
# only the subclass defines raised NoMethodError naming the subclass, which
# does define it (#4036).
#
# Two faults met here. The ivar's type was widened to poly in the class that
# DEFINES the reader and nowhere else, so the parent and the subclass laid the
# same inherited ivar out differently -- and an inherited ivar has one layout or
# nothing works through the parent pointer. That widening was superseded by the
# run-time dispatch #4023 added, and is gone.
#
# Then `name` itself: a user READER owns the name just as a user method does,
# and counting methods alone let Method#name / Class#name claim the call.
class Node
  attr_accessor :right
  def initialize
    @right = self
  end

  def link(other)
    other.right = self
    self
  end
end

class Column < Node
  attr_accessor :name
  def initialize(name)
    super()
    @name = name
  end
end

root = Column.new("root")
Column.new("a").link(root)
p root.right.name
p root.right.class
p root.right.right.name

# the same shape with a method rather than a reader
class Node2
  attr_accessor :peer
  def attach(other)
    other.peer = self
    self
  end
end
class Leaf < Node2
  def initialize(v) = @v = v
  def value = @v
end
b = Leaf.new(2)
Leaf.new(1).attach(b)
p b.peer.value

# a plain reader named `name`, with no inheritance in sight
class Plain
  attr_reader :name
  def initialize(n) = @name = n
end
p Plain.new("x").name
p [Plain.new("y")].first.name
