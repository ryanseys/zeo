# Ruby installs each `def` where it stands, and a MODULE's `def` is no
# different: a call written above a reopen answers the FIRST body, in every
# class that mixed the module in, in a module that includes it, and in an
# object that extended it.
#
# zeo used to answer the last body everywhere, because a module's rows reach
# a host by MATERIALIZATION -- analyze copies them onto each includer once,
# for the whole program -- so the reopen's body was the one that got copied
# and there was no position to install it at. `analyze::redefs` gives it the
# same timeline a plain class already had.

module M
  def z = "first"
end
class K; include M; end
class P; prepend M; end
obj = Object.new
obj.extend(M)

p K.new.z, P.new.z, obj.z
p K.instance_method(:z).owner

module M
  def z = "second"
end

p K.new.z, P.new.z, obj.z
p K.instance_method(:z).owner

# A host that joins AFTER the reopen sees the reopened body, and one with its
# OWN definition keeps it.
class Late; include M; end
p Late.new.z

class Over
  include M
  def z = "own"
end
module M
  def z = "third"
end
p Over.new.z, K.new.z

# `super` through the module reaches the body standing at the time.
module N
  def w = "n1"
end
class H
  include N
  def w = "h-" + super
end
p H.new.w
module N
  def w = "n2"
end
p H.new.w

# A module included into another module carries the timeline through.
module Inner
  def q = "i1"
end
module Outer
  include Inner
end
class Z; include Outer; end
p Z.new.q
module Inner
  def q = "i2"
end
p Z.new.q

# The shape that already worked, kept so a fix cannot close this by breaking
# that one.
class L
  def y = "first"
end
p L.new.y
class L
  def y = "second"
end
p L.new.y
__END__
"first"
"first"
"first"
M
"second"
"second"
"second"
M
"second"
"own"
"third"
"h-n1"
"h-n2"
"i1"
"i2"
"first"
"second"
