# A module whose method calls `super`, never included anywhere, so nothing says
# what that super answers. The raise emitted for it never returns, so its
# trailing value only has to type-check in the slot -- and the "0" it used
# stopped the build wherever the slot was a boxed one, an interpolation being
# the shape that found it (#4034).
# a module whose `super` is interpolated, never included
module Numbered
  def render(text) = "#{super}!"
end
class Plain
  def render(text) = text
end
p Plain.new.render("hi")

# the neighbouring shapes the report says already worked
module Whole
  def render2(text) = super
end
class Plain2
  def render2(text) = text
end
p Plain2.new.render2("ho")

# super interpolated where it DOES resolve
class Base
  def label = "base"
end
class Child < Base
  def label = "#{super}-child"
end
p Child.new.label

# an unresolved super in other value positions
module Orphan
  def a = [super]
  def b = super.to_s
  def c = { k: super }
  def d = super + 1
end
class Other
  def a = 1
  def b = 2
  def c = 3
  def d = 4
end
p [Other.new.a, Other.new.b, Other.new.c, Other.new.d]
