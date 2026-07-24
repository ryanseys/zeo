class Flash2
  def hi; "hi"; end
end
class CtrlA
  def f=(v); @f = v; end
end
class CtrlB
  def f=(v); @f = v; end
end
class Harness
  def dispatch(c)
    c.f = @cache ||= Flash2.new
  end
  def run
    dispatch(CtrlA.new)
    dispatch(CtrlB.new)
    @cache
  end
end
r = Harness.new.run
puts r.nil? ? "nil" : "ok: #{r.hi}"

class CA
  def x=(v); @x = v; end
end
class CB
  def x=(v); @x = v; end
end
class H2
  def dispatch_arr(c); c.x = @arr ||= [1, 2, 3]; end
  def dispatch_hash(c); c.x = @h ||= {"a" => 1}; end
  def run
    dispatch_arr(CA.new)
    dispatch_arr(CB.new)
    dispatch_hash(CA.new)
    [@arr.length, @h.size]
  end
end
p H2.new.run
