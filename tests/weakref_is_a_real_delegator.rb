# `WeakRef` is CRuby's own pure-Ruby file now (`gems/weakref`), so it really
# descends from `Delegator < BasicObject` -- which means a `WeakRef` is NOT an
# `Object`, the one consequence with a sharp edge (a guard sorted it the wrong
# way before). The builtin id is a namespace slot only: it exists so
# `WeakRef::RefError` has a scope, and the vendored file's `< Delegator`
# establishes the superclass.
require "weakref"

p WeakRef.superclass.to_s
p WeakRef.ancestors.map(&:to_s).include?("Delegator")
p WeakRef.ancestors.map(&:to_s).include?("Object")

referent = +"referent"
w = WeakRef.new(referent)
p w.is_a?(Delegator)
p w.is_a?(Object)
p w.upcase
p w.weakref_alive?
p w.respond_to?(:upcase)
p w.respond_to?(:no_such_method)
p w.__getobj__.equal?(referent)

# A `true`/`false`/`nil` referent cannot go in a WeakMap, so weakref.rb stashes
# it in an ivar -- which is why `weakref_alive?` answers `defined?`'s String.
t = WeakRef.new(true)
p t.weakref_alive?

# A user subclass runs its OWN initialize and reaches WeakRef's through super.
class TaggedRef2 < WeakRef
  def initialize(obj)
    super(obj)
    @tagged = true
  end

  def tagged? = @tagged
end
s = +"held"
r = TaggedRef2.new(s)
p r.class
p r.tagged?
p r.upcase
p r.weakref_alive?
p TaggedRef2.ancestors.map(&:to_s).first(3)

# Arity is the vendored file's own.
begin; WeakRef.new; rescue ArgumentError => e; p e.message; end
begin; WeakRef.new(1, 2); rescue ArgumentError => e; p e.message; end

# `WeakRef::RefError` is a StandardError, resolvable by name.
p WeakRef::RefError.superclass.to_s
p WeakRef::RefError.ancestors.map(&:to_s).include?("StandardError")

# `Delegator` and its concrete subclasses are untouched.
p SimpleDelegator.superclass.to_s
p DelegateClass(Hash).superclass.to_s
p SimpleDelegator.new([1, 2]).size
