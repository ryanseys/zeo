# An unresolved rhs (a NameError raise token, emitted as a diverging
# sp_RbVal expression) stored into a typed-value hash in VALUE position
# (the method tail) must coerce to the slot type so the emitted C
# typechecks; the raise fires before any read (#3256).
class K
  def initialize = @h = { "x" => "y" }
  def put(k, v)
    @h[k] = Undef.compute(v)
  end
end
begin
  K.new.put("a", "b")
rescue NameError => e
  puts e.class
end
