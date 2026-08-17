class Base; end
class Sub < Base; end

def kind(o)
  case o
  in Sub then "sub"
  in Base then "base"
  else "other"
  end
end
p kind(Sub.new)
p kind(Base.new)

class Other; end
o = Other.new
case o
in Base then p "wrong"
else p "right"
end

E = Data.define
r = (E.new in E)
p r
F = Data.define(:a)
r = (F.new(a: 1) in F)
p r
r = (F.new(a: 1) in E)
p r
G = Struct.new(:a)
r = (G.new(1) in G)
p r
E2 = Data.define
e2 = E2.new
case e2
in E2 then p "empty data match"
else p "empty data MISS"
end

F2 = Data.define(:a)
f2 = F2.new(a: 1)
case f2
in F2 then p "data match"
else p "data MISS"
end

G2 = Struct.new(:a)
g2 = G2.new(1)
case g2
in G2 then p "struct match"
else p "struct MISS"
end

class H2; end
h2 = H2.new
case h2
in H2 then p "class match"
else p "class MISS"
end
