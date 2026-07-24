P = Data.define(:x, :y)
p({ P.new(1, 2) => "a" }[P.new(1, 2)])
h = { P.new(1, 2) => "a" }; p(h[P.new(1, 2)])
