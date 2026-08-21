# The prepend transplant MOVED the module's method scope into the prepending
# class (`sc->class_id = ci`) rather than copying it. The same module prepended
# by more than one class then gave the first prepender the scope and left every
# later one with nothing to transplant: its prepend did nothing, silently, and
# the call fell through to the included chain (#4039). The include path clones
# for this reason among others.
module Shouted
  def render(text) = super.upcase
end
module Plain
  def render(text) = text
end

class A
  include Plain
  prepend Shouted
end
class B
  include Plain
  prepend Shouted
end
class C
  include Plain
  prepend Shouted
end
p [A.new.render("hi"), B.new.render("hi"), C.new.render("hi")]

# the reported shape: one class prepends alone, another prepends alongside a
# stack of includes
module Renderable
  def render(text) = text
end
module Trimmed
  def render(text) = super(text.strip)
end
module Bracketed
  def render(text) = "[#{super}]"
end
module Numbered
  def initialize(*args)
    super()
    @counter = 0
  end
  def render(text)
    @counter += 1
    "#{@counter}. #{super}"
  end
end

class Loud
  include Renderable
  include Trimmed
  prepend Shouted
end

class Everything
  include Renderable
  include Trimmed
  include Bracketed
  include Numbered
  prepend Shouted
end

p Loud.new.render("  hi  ")
p Everything.new.render("  hi  ")
p Everything.new.render("  ho  ")

# a prepended method with parameters, defaults and a block, cloned per class
module Tagged
  def label(text, sep = ":", &blk) = "<#{super(text, sep, &blk)}>"
end
module Basic
  def label(text, sep = ":") = "#{text}#{sep}"
end
class D
  include Basic
  prepend Tagged
end
class E
  include Basic
  prepend Tagged
end
p [D.new.label("d"), E.new.label("e", "!")]

# the module keeps its own identity for #owner-style questions
p A.new.is_a?(Shouted)
p A.ancestors.include?(Shouted)
