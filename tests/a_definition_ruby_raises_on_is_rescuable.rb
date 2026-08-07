# `superclass mismatch` is an EXCEPTION in ruby, raised at the definition -- not
# a broken program. A `begin ... rescue TypeError` around one catches it, prints,
# and carries on, so a program that does that has to compile and do the same.
#
# Unrescued, zeo refuses the compile instead of emitting a program that aborts:
# same problem, named earlier. That split is the whole point of this file -- the
# rescued half is what a compile-time-only rejection would have made impossible.

class A; end
class B < A; end

# A cycle: `A < B` while `B < A`. Ruby raises rather than closing the loop --
# and zeo must not build it either, because a superclass chain that points at
# itself is an infinite walk for every pass that follows it.
begin
  class A < B; end
rescue TypeError => e
  p e.message
end
p A.superclass
p B.superclass

# A plain conflict: two different superclasses for one name.
class D < String; end
begin
  class D < Array; end
rescue TypeError => e
  p e.message
end
p D.superclass

# The definition RAISED, so it changed nothing -- the class is exactly what it
# was, and the program keeps running.
p D.new.class
p B.new.is_a?(A)
p :after
