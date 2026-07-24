# Optional parameters on proc literals: CRuby's distribution -- requireds from
# the front, posts reserved at the back, optionals consume what remains (else
# their defaults, which may reference earlier params), the rest takes the rest.
p1 = proc do |a=5, *b, c|
  [a, b, c]
end
p p1.call(1, 2, 3)
p p1.call(1)
p p1.call(1, 2)
p2 = proc { |x, y=9| [x, y] }
p p2.call(1)
p p2.call(1, 2)
l = lambda { |x, y=9| [x, y] }
p l.call(3)
begin
  l.call
rescue ArgumentError
  puts "arity"
end
