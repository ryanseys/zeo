# `singleton_methods(false)` ignores its argument and always answers the
# inheriting list, so a subclass reports its parent's class methods as its own.
#
# `Kernel#singleton_methods` (crates/zeo-rt/src/builtins/kernel.rs) binds the
# optional argument as `_arg?` and never reads it; for a class receiver it
# always calls `dispatch::class_method_names`, which is the WALKING list --
# materialization copies a `def self.x` onto every subclass's registry entry,
# and the runtime overlay half now walks the ancestry too.
#
# Fix shape: `class_method_names` needs an own-only companion the way
# `instance_method_names` already takes a `VisFilter` and an `inherit` flag --
# the own list being the registry entry's `own_class_method_names` plus the
# receiver's OWN overlay, with no ancestor walk. Found while making a runtime
# `extend` reach subclasses (tests/issue_subclass_runtime_extend.rb); the
# inheriting answers there are all correct, only this narrowing is not.

class FBase
  def self.frozen_tag = "ft"
end
class FSub < FBase; end

p FSub.frozen_tag
p FSub.singleton_methods.include?(:frozen_tag)
p FBase.singleton_methods(false)
p FSub.singleton_methods(false)

module Store
  def tag = "tagged"
end
class RBase; end
RBase.extend Store
class RSub < RBase; end

p RSub.tag
p RSub.singleton_methods.include?(:tag)
p RSub.singleton_methods(false)
