# Two things a `class << self` body can hold that zeo used to reject outright,
# both straight out of irb: a nested `class` (color.rb's `class
# ColorizeVisitor < Prism::Visitor`), and a constant path rooted at `self`
# (input-method.rb's `self::Readline::HISTORY`).
#
# A class written here belongs to the SINGLETON class, exactly as a constant
# written here does -- `class X` IS a constant write with a body. Both are
# wrapped in the singleton surrogate at their own position, so the singleton
# methods beside them still reach them by bare name while the enclosing
# module answers for neither.
module Color
  class << self
    class Visitor
      def initialize(name) = @name = name
      def render = "<#{@name}>"
    end

    def paint(name)
      Visitor.new(name).render
    end
  end
end

puts Color.paint("red")

# The class is the singleton's, not the module's.
p Color.singleton_class.const_defined?(:Visitor)
p Color.constants
p(begin; Color::Visitor; rescue NameError => e; e.message; end)
p Color.singleton_class::Visitor.new("blue").render

# A constant beside it takes the same route, and the two list together.
module Sized
  class << self
    MAX = 5
    class Box; end
    def cap = MAX
  end
end
p Sized.cap
p Sized.singleton_class.constants.sort
p Sized.constants
p(begin; Sized::MAX; rescue NameError => e; e.message; end)

# `self::Engine.name` -- a constant path whose ROOT is dynamic but whose outer
# node is still a path, which is what the old "is the immediate PARENT a
# constant?" test misread as static.
class Reader
  class << self
    def setup
      const_set(:Engine, ::String)
      const_set(:LABEL, self::Engine.name)
    end
  end
end

Reader.setup
p Reader::Engine
p Reader::LABEL
__END__
<red>
true
[]
"uninitialized constant Color::Visitor"
"<blue>"
5
[:Box, :MAX]
[]
"uninitialized constant Sized::MAX"
String
"String"
