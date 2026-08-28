# An arm with no C VALUE cannot sit in a C conditional beside a typed sibling:
# a method whose body is `nil` compiles to a void function, and `a && a.analyze`
# put that call straight into one arm of `? :` whose other arm is an sp_Att *.
# The poly sibling boxed it, which is why only a TYPED sibling ever saw this --
# `a ? a.analyze : nil` worked while `a ? a.analyze : a` did not (#4163).
class Att
  def analyze = nil
  def name = "n"
  def count = 3
end

class Holder
  attr_accessor :att
  def ensure_analyzed
    att && att.analyze     # what `att&.analyze` lowers to
  end
end

a = Att.new
p(a && a.analyze)
p(a ? a.analyze : a)
p(a ? a.analyze : nil)
p(nil && a.analyze)
p(a && a.name)
p(a && a.count)
p((a || a.analyze).name)

h = Holder.new
h.att = a
p h.ensure_analyzed
h.att = nil
p h.ensure_analyzed

s = "x"
p(s && a.analyze)
n = 5
p(n && a.analyze)
