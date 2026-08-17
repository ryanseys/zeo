class P1; def greet; "P"; end; end
class Q1 < P1
  alias_method :old_greet, :greet
  def greet; "Q"; end
end
p Q1.new.old_greet

class P2; def greet; "P"; end; end
class Q2 < P2
  alias old_greet2 greet
  def greet; "Q"; end
end
p Q2.new.old_greet2   # Ruby: "P"   Spinel: "Q"

class A
  def greet; "A"; end
  alias_method :old_greet, :greet
  def greet; "B"; end
end
p A.new.greet       # => "B"
p A.new.old_greet   # => "A"

class C
  def greet; "C"; end
  alias_method :old_greet, :greet
  def greet; "D(" + old_greet + ")"; end
end
p C.new.greet       # => "D(C)"

class R1 < P1
  alias_method :parent_greet, :greet
  def greet
    "R(" + parent_greet + ")"
  end
end
p R1.new.greet
p R1.new.parent_greet

class S1 < P1
  alias_method :same_greet, :greet
end
p S1.new.same_greet
p S1.new.greet
