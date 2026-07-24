KW = Struct.new(:a, :b, keyword_init: true)
r = (begin; KW.new(a: 1, z: 9); "no error"; rescue ArgumentError => e; e.class; end); p r
p(KW.new(a: 1, b: 2).to_a)
p(KW.new(a: 1).to_a)
D = Data.define(:x, :y)
p((D.new(x: 1, w: 2) rescue $!.class))
