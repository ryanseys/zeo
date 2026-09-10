# Float#div by zero raises ZeroDivisionError.
#
# A Float that reaches a method through a boxed slot answers the whole
# Numeric surface, not only the operators (#3800).
rings = [[[[-63.1269583, 46.2338358], [-63.1260111, 46.234436]]]]
array = rings.flat_map { |ring| ring.first.map(&:first) }
p array.sum
p array.sum.div(array.size)

xs = [7.0, 3, -7.0]
a = xs[0]
b = xs[1]
c = xs[2]
p a.div(3)
p c.div(3)
p b.div(3)
p a.divmod(3)
p c.divmod(3)
p b.divmod(3)
p a.remainder(3)
p c.remainder(3)
p b.remainder(3)
p a.modulo(3)
p a.quo(3)
p a.fdiv(3)
p a.to_r
p a.coerce(3)
p b.coerce(2.5)
begin
  p a.div(0)
rescue ZeroDivisionError => e
  p e.class
end
begin
  p a.remainder(0)
rescue ZeroDivisionError => e
  p e.class
end
__END__
-126.2529694
-64
2
-3
1
[2, 1.0]
[-3, 2.0]
[1, 0]
1.0
-1.0
0
1.0
2.3333333333333335
2.3333333333333335
(7/1)
[3.0, 7.0]
[2.5, 3.0]
ZeroDivisionError
ZeroDivisionError
