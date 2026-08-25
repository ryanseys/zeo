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
# 4.0.6's. Ruby puts the class on the singleton class (`true` / `[]` /
# raises `uninitialized constant Color::Visitor`); zeo hands it to the
# enclosing module (`false` / `[:Visitor]` / `"<blue>"`). The `.divergence`
# sidecar records why, and `docs/COMPATIBILITY.md` carries the section.
p Color.singleton_class.const_defined?(:Visitor)
p Color.constants
p Color::Visitor.new("blue").render
