# The operator fast path stands down when a user reopen redefines the
# operator on the fast-path MRO, and BOTH numeric lanes gate the one
# decision: the boxed shape tests both tags, so a site cannot know which arm
# it will take.
#
# `Integer#<=>` is the oracle's own, so redefining `Numeric#<=>` below does
# NOT change what an Integer answers -- it is here because the redefinition
# still has to suppress the fast path, and the answers must stay ruby's.

class Numeric
  def <=>(other) = "num-cmp"
end

p(1 <=> 2)
p(1.5 <=> 2)
x = [3].first
p(x <=> 4)

# `Float#-` IS reached, and suppressing it must not change what Integer
# subtraction answers.
class Float
  def -(other) = "float-minus"
end
p(2.0 - 1.0)
p(5 - 1)
y = [7.5].first
p(y - 0.5)

# Operators nobody touched keep their native paths.
p(6 + 7)
p(6 * 7)
p(6 < 7)
p(2.5 + 0.25)
p([1, 2].include?(2))

# A redefinition reached through a variable receiver, not a literal.
def apply(a, b) = a <=> b
p apply(1, 2)
p apply(1.0, 2.0)
__END__
-1
-1
-1
"float-minus"
4
"float-minus"
13
42
true
2.75
true
-1
-1
