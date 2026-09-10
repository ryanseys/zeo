# A super call from a method in an extended module skips the extending
# object's own definition.
#
module Deco
  def render(text) = "[#{super}]"
end
module Loud
  def render(text) = super.upcase
end
class Plain
  def render(text) = text
end

o = Plain.new
o.extend(Deco)
p o.render("x")

q = Plain.new
q.extend(Loud)
p q.render("ab")

r = Plain.new
p r.render("plain")

module Adder
  def bump(n) = super + n
end
class Base
  def bump(n) = n * 10
end
b = Base.new
b.extend(Adder)
p b.bump(3)

module NoSuper
  def render(text) = "<#{text}>"
end
s = Plain.new
s.extend(NoSuper)
p s.render("y")
__END__
"[x]"
"AB"
"plain"
33
"<y>"
