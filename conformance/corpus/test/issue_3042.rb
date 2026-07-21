e = NoMethodError.new("msg", :meth, [1, 2])
p e.args
p e.name
p e.message
e2 = NoMethodError.new("m2", :other)
p e2.name
n = NameError.new("nm", :sym)
p n.name
