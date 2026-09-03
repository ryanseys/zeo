# `class Kid < Base` where Base is a Struct. The subclass is registered before
# the anonymous Struct it inherits from, and the member merge ran strictly in
# index order, so the subclass came out with no members at all -- its methods
# read fields the struct did not have. A Struct subclass is a Struct: it keeps
# the members, the positional constructor and the member face.
Base = Struct.new(:a, :b)
class Kid < Base
  def total = a + b
end
k = Kid.new(1, 2)
p k.total
p k.a
p k.b
p k.to_a
p k.members
p k.to_h
p k == Kid.new(1, 2)
p Kid.new(1, 2).inspect
p k[0]
p k.class

class Named < Struct.new(:x)
  def double = x * 2
end
n = Named.new(4)
p n.double
p n.x
p n.to_a

# the plain Struct is unchanged
p Base.new(5, 6).a
p Base.new(5, 6).to_a
__END__
3
1
2
[1, 2]
[:a, :b]
{a: 1, b: 2}
true
"#<struct Kid a=1, b=2>"
1
Kid
8
4
[4]
5
[5, 6]
