# Same-class and inherited sources, both spellings, chained across levels.

class Base
  def greet(who); "hi " + who; end
  alias hail greet
  alias_method :salute, :greet
end
class Mid < Base
  alias_method :welcome, :greet
  alias hey greet
end
class Leaf < Mid
  alias again welcome
end
puts Base.new.hail("a")
puts Base.new.salute("b")
puts Mid.new.welcome("c")
puts Mid.new.hey("d")
puts Leaf.new.again("e")
p Leaf.method_defined?(:again)
__END__
hi a
hi b
hi c
hi d
hi e
true
