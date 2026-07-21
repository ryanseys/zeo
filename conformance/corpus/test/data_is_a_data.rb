P = Data.define(:x, :y)
p(P.new(1, 2).is_a?(Data))
a = P.new(1, 2); p(a.is_a?(Data))
p(P.ancestors.include?(Data))
