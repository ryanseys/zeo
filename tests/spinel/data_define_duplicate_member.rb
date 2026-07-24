r1 = (begin; k1 = Data.define(:x, :x); "members=#{k1.members}"; rescue ArgumentError; "argerr"; end)
p r1
r2 = (begin; k2 = Struct.new(:a, :a); "ok"; rescue ArgumentError; "argerr"; end)
p r2
# no-dup definitions still work
Good = Data.define(:x, :y)
p Good.new(1, 2).x
p Good.members
