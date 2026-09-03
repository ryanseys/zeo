# A REAL bug found while adding this test: ivar-flattening originally
# only scanned the MRO-winning materialized methods, missing any ivar
# only ever touched through a `super`-reachable (but shadowed, not
# winning) ancestor body -- `B#initialize`/`C#initialize` both call
# `super` and their OWN bodies don't textually mention `@a`, only
# `A#initialize`'s spliced-in body does. Fixed by collecting ivars from
# EVERY ancestor's own methods, not just the ones that end up
# materialized as the winning definition.

class A
  def initialize
    @a = 1
  end
end
class B < A
  def initialize
    super
    @b = 2
  end
end
class C < B
  def initialize
    super
    @c = 3
  end
  def total
    @a + @b + @c
  end
end
puts C.new.total
__END__
6
