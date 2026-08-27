# A bare constant resolves in two halves: the lexical scopes' own tables, then
# the ANCESTORS of the innermost one. A superclass clause is a bare constant,
# so `class Leaf < Inner` nested in `class Mid < Base` finds `Base::Inner`.
#
# Zeo did only the first half for a superclass clause -- ordinary reads
# already did both -- so the definition resolved to nothing, was rewritten
# into a runtime `Leaf = Class.new(Inner)`, and the next line's `Leaf.new`
# raised `uninitialized constant`.
#
# rake writes exactly this: `class Scope < LinkedList` nests `class EmptyScope
# < EmptyLinkedList`, where `EmptyLinkedList` is a constant of `LinkedList`.
# `require "rake"` died on the `EMPTY = EmptyScope.new` two lines below it.

class Base
  class Inner
    def who = "inner"
  end
  module Marker; end
  WIDTH = 7
end

class Mid < Base
  class Leaf < Inner
    def who = "leaf"
  end
  ONE = Leaf.new
end

p Mid::Leaf.superclass
p Mid::Leaf.name
p Mid::ONE.who
p Mid::Leaf.ancestors.take(3).map(&:to_s)

# The lexical half still wins over the ancestry half.
class Shadow < Base
  class Inner
    def who = "shadowed"
  end
  class Leaf < Inner; end
end
p Shadow::Leaf.superclass.name
p Shadow::Leaf.new.who

# A module of that name in the ancestry is still a TypeError, as ruby says.
begin
  Class.new(Base) { const_set(:Bad, Class.new(Base::Marker)) }
rescue TypeError => e
  puts e.message
end
