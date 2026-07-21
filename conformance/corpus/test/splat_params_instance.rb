# Splat parameter (def f(*a)) on instance methods: the call site packs
# trailing args into the rest array instead of spilling each into its
# own slot (which produced "too many arguments to function").
class T
  def go(*a); a.size; end
  def withpre(x, *a); x + a.size; end
  def firstrest(*a); a[0]; end
  def sumrest(*a); s = 0; a.each { |v| s += v }; s; end
end
class Obj; def initialize(n); @n = n; end; def n; @n; end; end
class ObjSub < Obj; end  # heap, not value-typed
t = T.new
puts t.go(1, 2)
puts t.go(1, 2, 3, 4)
puts t.go
puts t.withpre(10, 1, 2, 3)
puts t.firstrest(7, 8, 9)
puts t.sumrest(1, 2, 3, 4)
xs = [5, 6, 7]
puts t.sumrest(*xs)
puts t.go(Obj.new(1), Obj.new(2))
