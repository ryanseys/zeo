# CRuby's `WeakRef < Delegator`. zeo's is a native class under `Object`, so
# the superclass, the ancestry and `is_a?(Delegator)` all answer differently,
# and a `rescue`/`case`/guard written against `Delegator` never matches.
#
# The gap is NARROWER than "no delegate library". zeo has `Delegator` and
# `SimpleDelegator`, and `DelegateClass(Hash)` answers a class whose
# superclass really is `Delegator` -- the shape
# `a_class_nested_in_a_runtime_minted_namespace.rb` is built on. Two things
# are missing, and only these two:
#
# 1. `require "weakref"` does not pull delegate.rb in. Requiring it FIRST does
#    not help either, which is what makes this a reparenting job and not a
#    require-graph one.
# 2. zeo's native `WeakRef` is registered under `Object` and never reparented.
#
# The consequence with the sharpest edge is `is_a?(Object)`: CRuby's
# `Delegator` descends from `BasicObject` and undefines most of Object, so a
# `WeakRef` is NOT an Object there and IS one here -- inverted, not merely
# absent. Any `obj.is_a?(Object)` guard sorts a WeakRef the wrong way.
#
# A fix reparents the native class onto the existing `Delegator` (and moves
# the `method_missing`/`respond_to_missing?` rows there, since Delegator owns
# them in CRuby). The delegation BEHAVIOUR is already right --
# `a_weakref_subclass_runs_its_own_initialize.rb` pins the constructor split
# and the forwarding.
require "delegate"
require "weakref"

p WeakRef.superclass.to_s
p WeakRef.ancestors.map(&:to_s).include?("Delegator")

# A strong reference kept on purpose: zeo's weak handles are `Arc::downgrade`d,
# so an unreferenced referent is collected the instant the literal goes out of
# scope, where MRI's GC has simply not run yet. That is a separate, documented
# divergence and would drown this one.
referent = +"referent"
w = WeakRef.new(referent)
p w.is_a?(Delegator)
p w.is_a?(Object)
p w.upcase

# What zeo DOES have, so a fix must not disturb it.
p SimpleDelegator.superclass.to_s
p DelegateClass(Hash).superclass.to_s
p SimpleDelegator.new([1, 2]).size
__END__
"Delegator"
true
true
false
"REFERENT"
"Delegator"
"Delegator"
2
