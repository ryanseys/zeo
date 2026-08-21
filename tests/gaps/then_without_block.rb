# GAP -- imported from the spinel corpus at fa06b601.
#
# `Kernel#then` (and `yield_self`) without a block answers `nil` where CRuby
# returns an ENUMERATOR of one element, so `1.then.first` is `1` in CRuby and
# a NoMethodError on nil in zeo.
#
# The block-less enumerator form is what makes `obj.then.each { }` and the
# lazy chains built on it work.
#
# The original header follows. It describes the PREDECESSOR project's
# version of this test and its own fix, not zeo's divergence above.
#
# `then` / `yield_self` given no block is an Enumerator over exactly one
# element, the receiver -- spinel refused the call outright (#4028):
#
#   undefined method 'then' for an instance of Array (NoMethodError)
#
# and a `then` whose block body is EMPTY declared its result slot `void`,
# because the desugared `then { nil }` types as nil, which has no C slot.
[].then{}
p([].then{})
p(5.then{})
p("x".then{})
p({a: 1}.then{})

e = 5.then
p e
p e.class
p e.to_a
p e.next
p e.size
p 5.yield_self.to_a

# the receiver is carried whatever it is -- a nil one is a real source, not the
# absence of one, which #inspect must not read as "fall back to the items"
p nil.then
p nil.then.to_a
p :sym.then.next
p 1.5.then.to_a
p((1..3).then.to_a)
p({a: 1}.then.to_a)
p [1, 2].then.to_a
p "s".yield_self.to_a

class Holder
  def initialize(v) = @v = v
  def v = @v
end
p Holder.new(3).then.next.v

# reached through a poly slot, where the receiver's class is a run-time question
p [1, [2, "x"]][1].then.to_a
p [1, [2, "x"]][0].then.next

# and it still composes with the enumerator surface
p 7.then.with_index.to_a
p [4].then.first
puts "OK"
