# `remove_method` on a MODULE takes the name away from every class that mixed
# the module in, and a class's OWN definition survives it.
#
# Analyze materializes a module's rows onto each host at compile time, so
# `Host.new.m` resolves through the HOST's own row and never asks the module.
# The tombstone `remove_method` writes is keyed on the module, at a position
# the lookup does not visit -- so the removal has to name the hosts and
# tombstone them too. A host whose winner is its OWN def is not one of them.
#
# The hosts are named BEFORE the module's own tombstone lands. "Whose copy is
# this" is `method_owner`, and once the overlay is live that walk SKIPS a
# removed position -- so writing first made the module stop being the owner
# and the sweep find nobody. It reproduced only from the SECOND removal in a
# program, because the first is what arms the overlay.
#
# Reflection reads the same tombstone, and by a different rule than `undef`:
# a removal empties one position and the walk carries on, so the name is
# dropped from what that ancestor contributes rather than claimed outright.

module S
  def u = "s"
end
class C
  include S
end
class D
  prepend S
end
class E
  include S
  def u = "own"
end
p [C.new.u, D.new.u, E.new.u]

S.send(:remove_method, :u)
def try(o) = (o.u rescue $!.class.to_s)
p [try(C.new), try(D.new), try(E.new)]

# A later definition on the module restores it everywhere, and still does not
# displace the host's own.
S.send(:define_method, :u) { "s3" }
p [C.new.u, D.new.u, E.new.u]

# Reflection agrees with dispatch on both sides of the removal.
module T
  def w = 1
end
class F
  include T
end
p [F.method_defined?(:w), F.instance_method(:w).owner.to_s]
T.send(:remove_method, :w)
p [
  F.method_defined?(:w),
  F.new.respond_to?(:w),
  T.instance_methods(false).map(&:to_s),
  F.instance_methods(false).map(&:to_s),
]

# Removing from the HOST names a method the host does not own.
class G
  include T
end
begin
  G.send(:remove_method, :w)
rescue NameError => e
  p e.class.to_s
end
__END__
["s", "s", "own"]
["NoMethodError", "NoMethodError", "own"]
["s3", "s3", "own"]
[true, "T"]
[false, false, [], []]
"NameError"
