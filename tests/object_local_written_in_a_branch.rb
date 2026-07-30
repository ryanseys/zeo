# A local holding a user-class instance, assigned somewhere other than a
# top-level statement of its scope. Zeo gives such a local an unboxed
# `Arc<Concrete>` slot whose binding comes from the assignment itself, so
# writing it inside an `if` arm scoped the binding to that arm and every read
# after the `if` named something that was not there -- the generated program
# did not compile at all. Fifteen lines of ordinary Ruby; irb's ext/loader.rb
# writes the same shape with a WorkSpace.
class W
  def initialize(x = nil)
    @x = x
  end
  def show = @x.inspect
end

def by_branch(c)
  if c
    ws = W.new(1)
  else
    ws = W.new
  end
  ws.show
end

puts by_branch(true)
puts by_branch(false)

# The same through a rescue clause, and through a block -- both are nested
# positions for the same reason.
def by_rescue(c)
  w = W.new(:fine)
  raise "no" if c
  w.show
rescue RuntimeError
  w = W.new(:rescued)
  w.show
end

puts by_rescue(false)
puts by_rescue(true)

def by_block(list)
  list.each do |n|
    w = W.new(n)
    puts w.show
  end
end

by_block([1, 2])

# A local assigned only at the top level of its scope keeps the fast path,
# which is the case this must not disturb.
def straight_line
  w = W.new(:direct)
  w.show
end

puts straight_line

# Assignment-as-expression still evaluates to the assigned value.
def chained
  a = (b = W.new(:shared))
  [a.show, b.show]
end

p chained
