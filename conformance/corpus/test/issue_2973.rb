syms = [:sx, :sy, :sz]
K = Data.define(*syms)
p K.members
k = K.new(1, 2, 3)
p [k.sx, k.sy, k.sz]
ms = [:a, :b]
S = Struct.new(*ms)
p S.members
p S.new(7, 8).to_a
