# `Struct.new(:a)` used as a VALUE rather than bound to a name -- printed,
# passed, put in an array -- had no class registered for it at all, and the
# call reported `Struct.new` as undefined (#4031). Only the forms that are a
# name's value or another call's receiver were registered.
#
# Such a class is anonymous: CRuby renders it as the address form, and the
# StructAnon_<n> the compiler keys it by is not a Ruby-visible name. #name
# already answered nil; #to_s and #inspect say the same thing now.
def anon?(s) = s.start_with?("#<Class:0x") && s.end_with?(">")

p anon?(Struct.new.to_s)
p anon?(Struct.new(:a).to_s)
p anon?(Struct.new(:a).inspect)
p anon?(Data.define(:x).to_s)
p Struct.new(:a).name
p Data.define(:x).name

s = Struct.new(:a, :b)
p anon?(s.to_s)
p s.name
p s.new(1, 2).to_a
p s.members

# a NAMED one keeps its name
S = Struct.new(:a)
p S.to_s
p S.name
p S.new(3).a

D = Data.define(:x)
p D.name
p D.new(x: 4).x

# and the chained forms that already worked
p Struct.new(:a).new(1).a
p Struct.new(:a).members
p [Struct.new(:a), Struct.new(:b)].size
p Struct.new(:a).new(7).class.name
