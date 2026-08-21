# An ivar's element type must not leak between classes that merely share the
# ivar's NAME. Ivar slots are per-class, but the two queries that disambiguate
# `<<` (Array#push vs Integer#<< shift vs a user-defined operator) index writes
# by name and then filter by SCOPE for a local and by nothing for an ivar. So a
# numeric `@threads = 1` in one class answered a question asked about a
# completely unrelated `@threads` in another: the push promotion was declined
# there, the bare `[]` never learned its element type, and the slot kept the
# empty literal's default int array. Pushing a pointer element into it then
# failed at `cc` with an int-conversion error.
#
# Same family as the `strbuf_mut_kind` gate: a name-keyed table cannot tell
# `out << "x"` from `b0 << 4`, so every use needs the owner as part of the key.

class Config
  def initialize
    @threads = 1
  end
  def n
    @threads
  end
end

class Pool
  def initialize
    @threads = []
    @threads << "w1"
    @threads << "w2"
  end
  def names
    @threads
  end
end

p Config.new.n
p Pool.new.names

# The owner can also be the top-level main object, whose ivars live outside any
# class -- it must not collide with the first real class either.
@threads = 3
class Box
  def initialize
    @threads = []
    @threads << "b"
  end
  def items
    @threads
  end
end

p @threads
p Box.new.items

# The guard the above narrows still has to fire WITHIN one class: `@bits` really
# is numeric here, so `@bits << 3` is Integer#<< (a shift), not a push. Getting
# this wrong is silent -- a wrong answer, not a compile error.
class Shifter
  def initialize
    @bits = 1
  end
  def shl
    @bits = @bits << 3
    @bits
  end
end

p Shifter.new.shl
