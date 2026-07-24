# A Data instance is frozen by construction and stays so through every copy:
# dup and clone(freeze: false) both return a frozen value in CRuby (#2716).
# Struct copies keep the ordinary rules (dup never freezes).
P = Data.define(:x, :y)
a = P.new(1, 2)
p(a.dup.frozen?)
p(a.clone(freeze: false).frozen?)
p(a.clone.frozen?)
p(a.dup.x)
S = Struct.new(:v)
s = S.new(5)
p(s.dup.frozen?)
p(s.clone.frozen?)
