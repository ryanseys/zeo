# Method/UnboundMethod reflection: ==/eql?, original_name, parameters,
def dbl(n) = n * 2
class C
  def m(a, b = 1, *r, k: 2, **kw, &blk) = a
  alias_method :m2, :m
end
m1 = method(:dbl)
m2 = method(:dbl)
p m1 == m2
p m1.eql?(m2)
p m1.original_name
p m1.parameters
p m1.clone.arity
u = C.instance_method(:m)
p u.parameters
p C.instance_method(:m2).original_name
