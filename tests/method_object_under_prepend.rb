# A `Method` or `UnboundMethod` names ONE entry in the chain. Under `prepend`,
# zeo loses which one:
#
#   * `super_method` does not advance past the prepended module: it answers a
#     Method with the SAME owner, so the idiomatic `while m; m = m.super_method;
#     end` walk never terminates. The bounded loop below shows twenty repeats of
#     `Mixin` where ruby reaches `Sub` then `Base` and stops.
#   * `Base.instance_method(:greet).bind(sub)` re-dispatches from the top of the
#     receiver's MRO -- answering "mixin+sub+base" -- where ruby runs `Base`'s
#     body alone and answers "base".
#
# `bind` is the sharper half: an UnboundMethod taken from a named ancestor is
# how you deliberately bypass an override (`Base.instance_method(:x).bind_call`
# is the standard way to defeat a `method_missing` proxy or a prepended
# wrapper), so re-dispatching gives back exactly what the caller asked to skip.
#
# `tests/method_object_freezes_its_entry.rb` fixed the sibling case -- a Method
# whose entry is REDEFINED afterwards -- by freezing the layer the name resolved
# through. This is the same question asked of a chain position the caller named
# explicitly rather than one that was resolved for them.

class Base
  def greet = "base"
end
module Mixin
  def greet = "mixin+" + super
end
class Sub < Base
  prepend Mixin
  def greet = "sub+" + super
end

m = Sub.new.method(:greet)
p m.owner
p m.super_method.owner
p m.super_method.super_method.owner
p m.call

p Base.instance_method(:greet).bind(Sub.new).call
p Base.instance_method(:greet).bind_call(Sub.new)
p Sub.instance_method(:greet).owner
p Mixin.instance_method(:greet).owner

# The chain walk, bounded so this file terminates under zeo.
walk = []
cur = Sub.new.method(:greet)
20.times do
  break unless cur
  walk << cur.owner.to_s
  cur = cur.super_method
end
p walk
