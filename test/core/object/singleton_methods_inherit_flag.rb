# The optional `inherit` argument on the singleton-listing family.
#
# Narrowing has to reach all three places a class method can live: the
# compile-time registry (where materialization copies every inherited
# `def self.x` onto each subclass), the runtime overlay (which an `extend`
# writes onto the extended class alone), and the builtin class-method table.
#
# `methods(false)` is the trap: CRuby answers `singleton_methods(false)` from
# it, dropping the class's instance methods entirely rather than narrowing
# them, so it cannot share `public_methods`' body.

class FBase
  def self.frozen_tag = "ft"
  def inst = "i"
end
class FSub < FBase
  def self.sub_tag = "st"
end

p FSub.frozen_tag
p FSub.singleton_methods.include?(:frozen_tag)
p FBase.singleton_methods(false)
p FSub.singleton_methods(false)

# `false` and `nil` both narrow; every other argument, and none at all, does not.
p FSub.singleton_methods(nil)
p FSub.singleton_methods(true).sort
p FSub.singleton_methods.sort

# `methods(false)` IS the singleton list -- no instance methods at all.
p FSub.methods(false)
p FSub.new.methods(false)
p FBase.new.methods(false)

module Store
  def tag = "tagged"
end
class RBase; end
RBase.extend Store
class RSub < RBase; end

p RSub.tag
p RSub.singleton_methods.include?(:tag)
p RSub.singleton_methods(false)
p RBase.singleton_methods(false)
p RSub.methods(false)

# A `define_singleton_method` lands on the receiver alone, the same way.
class DBase; end
DBase.define_singleton_method(:minted) { "m" }
class DSub < DBase; end
p DSub.minted
p DBase.singleton_methods(false)
p DSub.singleton_methods(false)

# `private_class_method` keeps a name off both widths.
class PBase
  def self.hidden = "h"
  private_class_method :hidden
end
p PBase.singleton_methods(false)
p PBase.methods(false)

# The instance-side flags are unaffected: they narrow, they do not empty.
p FSub.new.private_methods(false).include?(:puts)
p FSub.instance_methods(false)
p FSub.new.public_methods(false).include?(:inst)
__END__
"ft"
true
[:frozen_tag]
[:sub_tag]
[:sub_tag]
[:frozen_tag, :sub_tag]
[:frozen_tag, :sub_tag]
[:sub_tag]
[]
[]
"tagged"
true
[]
[]
[]
"m"
[:minted]
[]
[]
[]
false
[]
false
