# zeo's divergence: a receiverless `eval(x)` inside a class that DEFINES its
# own `eval` is taken for Kernel#eval -- the eval recognizer in lower/parse
# runs before sibling-method resolution -- so the recursive call at line 14
# coerces its argument to String and raises TypeError ("no implicit conversion
# of Integer into String") where ruby runs Interp#eval. The explicit-receiver
# spelling (`i.eval(3)`) already dispatches correctly, which is what makes the
# recognizer the cause. (Imported from spinel, which had the same bug for a
# different reason; its Kernel#eval-on-a-runtime-string refusal is separate
# and stays.)
class Interp
  def initialize
    @visits = 0
  end

  def eval(node)
    @visits += 1
    if node.is_a?(Array)
      eval(node[0]) + eval(node[1])
    else
      node * 2
    end
  end

  def visits
    @visits
  end
end

i = Interp.new
p i.eval(3)
p i.eval([1, 2])
p i.eval([[1, 2], 3])
p i.visits

# an explicit receiver resolves the same way
p i.eval(5)
p Interp.new.eval([4, 5])

# a subclass inherits it
class Sub < Interp
  def twice(n)
    eval(n) + eval(n)
  end
end
p Sub.new.twice(6)
