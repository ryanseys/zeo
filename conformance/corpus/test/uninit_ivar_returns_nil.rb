# Issue #712. A read-only @ivar (never assigned anywhere in the class)
# must read as nil, not fail with "struct has no field iv_x". The scan
# pass now adds a "nil" slot for any read-only ivar so the struct gets
# the field and Ruby semantics surface for `.inspect` / `.nil?`.

class C
  def x; @x; end
  def y_nilp; @y.nil?; end
end
puts C.new.x.inspect
puts C.new.y_nilp

# When a method writes the ivar but another method reads it before
# write, the slot still has to widen correctly across writers. Here
# `@n` gets a string write so reads must see "" or the written value.
class D
  def set; @n = "x"; end
  def get; @n; end
end
d = D.new
puts d.get.inspect
d.set
puts d.get
