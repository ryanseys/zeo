# The poly `join` dispatch grew a user-class arm in ceaea73e (#4083's fix). That
# arm passed the boxed receiver as a POINTER; a value-type class takes self by
# value, and the C build stopped (#4091). The sibling arm at the default
# dispatch has dereferenced it since #2441.
#
# The class has to STAY a value type for this to bite, so nothing here hands a
# P around by reference -- an extra `show(P.new(...))` was enough to take it
# off the value-type path and the test stopped covering the bug.

class P
  def initialize(b)
    @b = b
  end

  def join(part)
    P.new(@b + "/" + part)
  end

  def to_s
    @b
  end
end

def show(v)
  v.join(", ")
end

puts show([1, 2])
puts show(["a", "b"])
puts P.new("x").join("y").to_s
