# `undef` works on a name this class only INHERITS, which is why it
# can't be "delete the local def" -- there is none. It stays live on
# the ancestor that defined it, and `respond_to?` must agree.

class B
  def inherited_m; "from B"; end
  def own; "own"; end
end
class C < B
  def local_m; "local"; end
  undef local_m
  undef inherited_m
end
begin; C.new.local_m; rescue NoMethodError; puts "local: NoMethodError"; end
begin; C.new.inherited_m; rescue NoMethodError; puts "inherited: NoMethodError"; end
p B.new.inherited_m
p C.new.own
p C.new.respond_to?(:inherited_m)
class D
  def a; 1; end
  def b; 2; end
  undef a, b
end
begin; D.new.a; rescue NoMethodError; puts "D#a gone"; end
__END__
local: NoMethodError
inherited: NoMethodError
"from B"
"own"
false
D#a gone
