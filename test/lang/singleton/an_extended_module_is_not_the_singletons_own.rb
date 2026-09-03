# `obj.extend(M)` files M's rows as the singleton class's OWN methods, where
# CRuby puts M in the singleton's SUPER chain. So
# `obj.singleton_class.instance_methods(false)` lists names that belong to the
# module.
#
# zeo copies the module's bodies into the object's identity-keyed singleton
# table (`extend_object_default`, `runtime_meta/api.rs`) and records the
# provenance separately in `extended_names` -- the same split the CLASS side
# makes with `extended_class_methods`, where `singleton_methods(false)` already
# consults it. The object side records the provenance and no reader asks.
#
# `singleton_methods` is deliberately NOT part of this: CRuby's wide form does
# report an extended module's methods, and only the narrow `false` form skips
# them.

def try(label)
  p [label, yield]
rescue NoMethodError, NameError => e
  p [label, :raised, e.class.to_s]
end

module Plain
  def pn = :pn
end
r = Object.new.extend(Plain)
try(:own_im)     { r.singleton_class.instance_methods(false) }
try(:anc)        { r.singleton_class.ancestors[1].to_s }
try(:wide_sing)  { r.singleton_methods }
try(:call)       { r.pn }

# An own `def obj.x` beside the extend stays own.
def r.mine = :mine
try(:own_after)  { r.singleton_class.instance_methods(false) }

module Plain2
  def qn = :qn
end
s = Object.new
s.extend(Plain, Plain2)
try(:two_own)    { s.singleton_class.instance_methods(false) }
try(:two_anc)    { s.singleton_class.ancestors[1, 2].map(&:to_s) }
__END__
[:own_im, []]
[:anc, "Plain"]
[:wide_sing, [:pn]]
[:call, :pn]
[:own_after, [:mine]]
[:two_own, []]
[:two_anc, ["Plain", "Plain2"]]
