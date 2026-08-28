# `method_added` reaching a class through `extend`, both standard spellings.
#
# The one that used to fail is the ClassMethods idiom: a module's `included`
# hook doing `base.extend(ClassMethods)`, with `method_added` defined in
# `ClassMethods`. The edge is written inside a method that runs at mixin
# time, so nothing static saw it and `analyze::def_hooks` found no hook body
# to announce to. `analyze::methods::extends_from_included_hook` reads the
# hook now, at the include.
#
# Kept beside it: the INHERITED spelling, which always worked. Two shapes of
# one feature belong in one file, so a change that fixes one and breaks the
# other cannot pass.
module Hook
  def self.included(base) = base.extend(ClassMethods)

  module ClassMethods
    def method_added(name)
      (@added ||= []) << name
    end

    def added = @added
  end
end

class Uses
  include Hook
  def a; end
  def b; end
end

p Uses.added

# The inherited spelling, which DOES work, so the two are side by side.
class Base
  def self.method_added(name) = ((@seen ||= []) << name)
  def self.seen = @seen
end

class Child < Base
  def one; end
end

p Child.seen
