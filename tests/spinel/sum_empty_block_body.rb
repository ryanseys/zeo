# `sum {}` has no statements, so the fold that lowers a block sum found no tail
# to type and refused the call, which then reached the run-time dispatch and
# raised NoMethodError. An empty body answers nil for every element: an empty
# receiver never runs it and keeps the 0 it starts from, and a non-empty one
# raises on the first nil, as CRuby's "nil can't be coerced" does.
a = []
p a.sum {}
p [].sum {}

b = [1]
begin
  p b.sum {}
rescue => e
  p e.class
end

c = ["x"]
begin
  p c.sum {}
rescue => e
  p e.class
end

# a block that does answer something is unaffected
p [1, 2].sum { |x| x }
p [1.5, 2.5].sum { |x| x }
p [1, 2].sum { 1 }
p [].sum
p [1, 2].sum(10) { |x| x }
