# `Kernel#then` / `yield_self` without a block is an Enumerator over one
# element -- the receiver -- and it knows its own SIZE.
#
# A `then` whose block body is empty is covered too: it answers nil rather
# than nothing at all.

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
__END__
nil
nil
nil
nil
#<Enumerator: 5:then>
Enumerator
[5]
5
1
[5]
#<Enumerator: nil:then>
[nil]
:sym
[1.5]
[1..3]
[{a: 1}]
[[1, 2]]
["s"]
3
[[2, "x"]]
1
[[7, 0]]
[4]
OK
