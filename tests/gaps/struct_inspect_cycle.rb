S = Struct.new(:x)
s = S.new(nil)
s.x = s
p s
