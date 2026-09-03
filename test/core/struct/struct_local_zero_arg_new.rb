# A member-less `Struct`/`Data` inspects with the space CRuby prints.
#
# CRuby writes `#<struct ` and then the NAME, so the separator belongs to
# the name's position: a named memberless struct is `#<struct Empty>` and
# an ANONYMOUS one keeps the space the name would have gone in
# (`#<struct >`). zeo dropped it whenever the member list was empty.
#
# The original header follows. It describes the PREDECESSOR project's
# version of this test and its own fix.
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
__END__
#<struct >
#<data >
#<struct >
[]
0
#<struct a=nil>
#<struct a=1>
1
nil
#<struct a=1, b=nil>
[1, 2]
#<struct S9>
#<struct S8 a=nil>
