def dbl(n) = n * 2
p(method(:dbl).clone.call(5))
p(method(:dbl).dup.call(5))
m = method(:dbl); c = m.clone; p c.call(5)
p(method(:dbl).call(7))
class K
  def tw(x) = x + x
end
mm = K.new.method(:tw)
p(mm.clone.call(8))
p(mm.dup.call(9))
