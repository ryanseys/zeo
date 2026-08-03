# `include`/`prepend`/`extend` are the PUBLIC verbs. Each is defined in terms of
# a private primitive that does the actual work -- `append_features`,
# `prepend_features`, `extend_object` -- and calls the notification hook after:
#
#     rb_funcall(mod, append_features, 1, base)
#     rb_funcall(mod, included, 1, base)
#
# The insertion happens ONLY inside the default primitive. So a module that
# overrides one and omits `super` skips the mixin entirely -- and `included`
# still fires, because the two calls are unconditional. That is what lets a
# module police how it is mixed in; the `singleton` gem is built on it.

puts "== prepend_features with super"
module PF
  def self.prepend_features(base) = (puts("PF.prepend_features(#{base})"); super)
  def self.prepended(base) = puts("PF.prepended(#{base})")
  def hi = "from PF"
end
class Host
  prepend PF
  def hi = "from Host"
end
p [Host.ancestors.first(3), Host.new.hi]

puts "== prepend_features WITHOUT super skips the mixin, but still notifies"
module NS
  def self.prepend_features(base) = puts("NS.prepend_features, no super")
  def self.prepended(base) = puts("NS.prepended")
  def nope = 1
end
class H2
  prepend NS
end
p [H2.ancestors.first(3), H2.new.respond_to?(:nope)]

puts "== append_features"
module AF
  def self.append_features(base) = (puts("AF.append_features(#{base})"); super)
  def yo = "from AF"
end
class H3
  include AF
end
p [H3.ancestors.include?(AF), H3.new.yo]

puts "== append_features without super"
module AN
  def self.append_features(base) = puts("AN.append_features, no super")
  def gone = 1
end
class H4
  include AN
end
p [H4.ancestors.include?(AN), H4.new.respond_to?(:gone)]

puts "== a module with NO override is still folded at compile time"
module Plain
  def plain = 1
end
class H5
  include Plain
end
p [H5.ancestors.include?(Plain), H5.new.plain]

puts "== extend_object"
module EO
  def self.extend_object(obj) = (puts("EO.extend_object(#{obj.class})"); super)
  def self.extended(obj) = puts("EO.extended(#{obj.class})")
  def tagged = "from EO"
end
o = Object.new
o.extend(EO)
p [o.tagged, o.is_a?(EO)]
# The optparse shape: the primitive's `super` is followed by setting an ivar
# on the receiver, which for a bare Array needs storage beside the value.
module Tagger
  def self.extend_object(obj)
    super
    obj.instance_eval { @tag = "set" }
  end
  def tag = @tag
end
arr = [1, 2]
arr.extend(Tagger)
p [arr.tag, arr.instance_variables, arr.is_a?(Tagger), arr]

puts "== extend_object without super skips the mixin, but still notifies"
module EN
  def self.extend_object(obj) = puts("EN.extend_object, no super")
  def self.extended(obj) = puts("EN.extended")
  def missing = 1
end
o2 = Object.new
o2.extend(EN)
p [o2.respond_to?(:missing), o2.is_a?(EN)]

puts "== a class body `extend` routes through it too"
class H6
  extend EO
end
p [H6.tagged, H6.singleton_class.include?(EO)]

puts "== the runtime shapes route through the primitive too"
# A prepend has to outrank the target's OWN method even when that method is
# defined afterwards -- ruby keeps the prepended module in its own layer.
K = Class.new { prepend PF; def hi = "from K" }
p K.new.hi
K2 = Class.new { include AN }
p K2.new.respond_to?(:gone)
