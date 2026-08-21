# `obj.define_singleton_method(:m) { }` compiled on a CONSTANT receiver and
# stopped the build on a LOCAL one -- the same object, the same synthesized
# singleton subclass, and `def obj.m` on that local works. The emitter
# re-derived "is this a singleton receiver?" from the receiver's type, and a
# local that also carries a `def obj.m` has widened to poly by then, so the
# call reached the unsupported-feature diagnostic.
class K
  def base = "base"
end

k = K.new
k.define_singleton_method(:dsm) { "dsm-local" }
p k.dsm
p k.base

# alongside a def on the same local: both land on one subclass
d = K.new
def d.plain = "plain"
d.define_singleton_method(:also) { "also" }
p d.plain
p d.also

# the constant spelling, unchanged
CFG = K.new
CFG.define_singleton_method(:on_const) { "const" }
p CFG.on_const

# and the rest of the singleton surface still resolves
e = K.new
def e.extra = "extra"
p e.extra
class << e
  def blockform = "bf"
end
p e.blockform
module M
  def from_mod = "mod"
end
f = K.new
f.extend(M)
p f.from_mod
p f.base
