# A poly receiver dispatching to a method that takes `&blk`.
#
# The dispatch built its arm from the parameter list alone, and a `&blk`
# parameter is not in it -- so the call went out one argument short of the
# function it named. Ill-typed C, caught only by -Werror.
#
# The proc is materialized once, ahead of the switch, and shared by every arm:
# only one arm runs, so building it per candidate class would allocate a proc
# per class for nothing.

class A
  def go(&blk)
    blk ? 10 : -1
  end

  def label
    "A"
  end
end

class B
  def go(&blk)
    blk ? 20 : -2
  end

  def label
    "B"
  end
end

def pick(n)
  n == 1 ? A.new : B.new
end

p pick(1).go { |v| v }
p pick(2).go { |v| v }
p pick(1).go
p pick(2).go

# the same dispatch with no block involved still works
p pick(1).label
p pick(2).label

# a method taking both parameters and a block
class P
  def run(a, b, &blk)
    blk ? a + b : a - b
  end
end

class Q
  def run(a, b, &blk)
    blk ? a * b : 0
  end
end

def pick2(n)
  n == 1 ? P.new : Q.new
end

p pick2(1).run(3, 4) { |x| x }
p pick2(2).run(3, 4) { |x| x }
p pick2(1).run(3, 4)
p pick2(2).run(3, 4)
