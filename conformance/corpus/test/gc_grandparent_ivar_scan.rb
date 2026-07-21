# Grandparent ivar must survive GC even when a passthrough middle
# class introduces no ivars of its own. The leaf's scan function has
# to walk the full ancestor chain, not just the immediate parent.
class A
  def payload=(v); @payload = v; end
  def payload; @payload; end
end
class B < A          # passthrough: no ivars of its own
end
class C < B          # leaf with its own ivar
  def own=(v); @own = v; end
  def own; @own; end
end

def churn(n)
  s = ""; i = 0
  while i < n; s = s + i.to_s + ","; i += 1; end
  s.length
end

obj = C.new
obj.payload = "secret_" + 42.to_s
obj.own     = "kept_" + 7.to_s
GC.start
churn(8000)
GC.start
puts "own=" + obj.own
puts "payload=" + obj.payload
