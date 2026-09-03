# `Parent#greet` calls `super`, reaching `Middle#greet` (an included
# module), which itself calls `super`, reaching `GrandParent#greet` --
# a chain of 3, verifying `emit_super_inline`'s ancestors-based search
# composes correctly across more than one hop and a mixed
# class/module boundary.

class GrandParent
  def greet
    "grandparent"
  end
end
module Middle
  def greet
    "middle(#{super})"
  end
end
class Parent < GrandParent
  include Middle
  def greet
    "parent(#{super})"
  end
end
puts Parent.new.greet
__END__
parent(middle(grandparent))
