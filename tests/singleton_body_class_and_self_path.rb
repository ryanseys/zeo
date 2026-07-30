# Two things a `class << self` body can hold that zeo used to reject outright,
# both straight out of irb: a nested `class` (color.rb's `class
# ColorizeVisitor < Prism::Visitor`), and a constant path rooted at `self`
# (input-method.rb's `self::Readline::HISTORY`).
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

# ---- DELIBERATE DIVERGENCE; the lines below are ZEO's answers, not ruby
# 4.0.6's. A class written in a `class << self` body belongs to the SINGLETON
# class, so the oracle says `false` / `[]` / raises `uninitialized constant
# Color::Visitor`. Zeo hands the definition to the enclosing module, which
# keeps the property the code depends on -- the singleton methods beside it
# reach it by bare name, as `paint` does above -- and pays for it by also
# answering the qualified lookup. See `docs/COMPATIBILITY.md`.
p Color.singleton_class.const_defined?(:Visitor)
p Color.constants
p Color::Visitor.new("blue").render
