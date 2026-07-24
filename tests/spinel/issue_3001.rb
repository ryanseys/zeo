h = {a: 1, b: 2}.freeze
p (h.delete(:a) rescue $!.class); p h
p (h.clear rescue $!.class)
p (h.merge!({c: 3}) rescue $!.class)
p (h.replace({z: 9}) rescue $!.class)
g = {a: 1}
p g.delete(:a); p g
