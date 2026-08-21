# GAP -- imported from the spinel corpus at fa06b601.
#
# A member-less `Struct`/`Data` inspects without the space CRuby prints:
#
#   CRuby  "#<struct >"    zeo  "#<struct>"
#
# CRuby writes the tag, then a space, then the (empty) member list, and does
# not special-case the empty one. Cosmetic, and exactly the kind of thing a
# golden pins.
#
# The original header follows. It describes the PREDECESSOR project's
# version of this test and its own fix, not zeo's divergence above.
#
# `.new` with no arguments on a Struct held in a LOCAL was claimed by the
# runtime-class switch, which a local statically holding one class should never
# reach -- it dispatches like the constant it holds. That switch also has no arm
# for a Struct, so it came out empty and its nil seed was cast to the struct
# pointer. A memberless anonymous struct also inspects with the space that would
# precede its first member, as CRuby does.
p Struct.new.new
p Data.define.new

s = Struct.new
p s.new
x = s.new
p x.to_a
p x.size

t = Struct.new(:a)
p t.new
p t.new(1)
p t.new(1).a
p t.new.a

u = Struct.new(:a, :b)
p u.new(1)
p u.new(1, 2).to_a

S9 = Struct.new
p S9.new
S8 = Struct.new(:a)
p S8.new
