class N
  def deconstruct_keys(keys) = { kind: :binop, n: 1 }
end
r = (N.new in { kind: :binop })
p r
r = (N.new in { kind: :other })
p r
r = (N.new in { kind: :binop, n: 1 })
p r

class M
  def deconstruct = [1, 2]
end
r = (M.new in [1, 2])
p r
r = (M.new in [1, 3])
p r

S = Struct.new(:kind, :n)
r = (S.new(:binop, 1) in { kind: :binop })
p r
r = (S.new(:binop, 1) in { kind: :other })
p r

D = Data.define(:kind)
r = (D.new(kind: :binop) in { kind: :binop })
p r

case N.new
in { kind: :binop } then p "case ok"
else p "case miss"
end

r = ({ kind: :binop } in { kind: :binop })
p r
r = ([1, 2] in [1, 2])
p r
