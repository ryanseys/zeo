# Value-position forms: a.v ||= / &&= evaluates to the attribute's value
# (nullable-int slots key on the nil sentinel); `def` evaluates to :name;
# a dynamic regexp compiles in any value position.
class A
  def initialize
    @v = nil
    @w = [1]
  end
  def set
    @v = @w[5]
  end
  attr_accessor :v
end
a = A.new
a.set
x = (a.v ||= 5)
p x
p a.v
y = (a.v &&= 9)
p y
p a.v
p(def foo; end)
xx = "b"
r = /a#{xx}c/
p("abc" =~ r)
p("zzz" =~ r)
