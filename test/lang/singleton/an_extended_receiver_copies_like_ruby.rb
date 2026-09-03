# How extensions travel across copies: an object's clone carries the
# singleton class (extended modules and def obj.method rows) and dup
# drops it; a class's dup AND clone both keep it (rb_mod_init_copy
# clones the singleton class either way).
module Ex
  def exx = "exx"
end
o = Object.new
o.extend Ex
def o.own = "own"
c = o.clone
p c.exx
p c.own
p c.is_a?(Ex)
p c.singleton_class.ancestors.include?(Ex)
d = o.dup
p d.respond_to?(:exx)
p d.respond_to?(:own)
k = Class.new
k.extend Ex
kc = k.clone
p kc.exx
p kc.is_a?(Ex)
kd = k.dup
p kd.respond_to?(:exx)
p kd.is_a?(Ex)
__END__
"exx"
"own"
true
true
false
false
"exx"
true
true
true
