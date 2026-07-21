# A local assigned `Class.new` inside a top-level method body resolves
# as an instance_exec receiver. The rewrite now walks top-level method
# bodies (ieval_walk_toplevel_methods), the sibling of the class-method
# walk, so the receiver's class flows through scan_locals_first_type.

class Tally
  def initialize
    @sum = 0
  end

  def add(n)
    @sum = @sum + n
  end

  def sum
    @sum
  end
end

def accumulate
  t = Tally.new
  t.instance_exec { add(3); add(4); sum }
end

puts accumulate     #=> 7
puts "done"
