# A Ruby method named `clone` becomes an INHERENT `fn clone` on the
# generated struct; every internal self/local copy must therefore avoid
# `.clone()` method syntax (which would resolve to the Ruby method --
# infinite recursion in `clone`'s own body, type errors elsewhere).
# Exercises the collision through self-reference, ivar writes (the
# frozen-check emission), and an Object-typed local re-read.

class Uncopyable
  def initialize
    @n = 1
  end
  def clone
    raise TypeError, "can't clone #{self.class}"
  end
  def bump
    @n += 1
    self
  end
  def n
    @n
  end
end
u = Uncopyable.new
u.bump
puts u.n
begin
  u.clone
rescue TypeError => e
  puts e.message
end
__END__
2
can't clone Uncopyable
