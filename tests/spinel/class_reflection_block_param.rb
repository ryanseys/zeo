# `ancestors` (and its Module neighbours) had an arm only for a receiver typed
# TY_CLASS -- a constant. Iterating an Array of classes gives the block a BOXED
# class, which reached no arm at all: the call reported the method as undefined,
# and with nothing to type it, anything chained on reported itself undefined
# "for unknown" (#4018).
class A; end
class B; end
module M; end
class C
  include M
end

[A, B].each do |klass|
  p klass.ancestors.map { |m| m.name.to_s }
end

[A, C].each do |klass|
  p klass.ancestors
  p klass.superclass
  p klass.included_modules
  p klass.ancestors.length
  p klass.ancestors.take_while { |m| true }.size
end

# the constant receiver keeps answering the same thing
p A.ancestors
p A.superclass
p C.included_modules
p A.ancestors.map { |m| m.name }

# and a class reached through a local or a hash value is the same boxed value
k = [A].first
p k.ancestors
h = { a: A }
p h[:a].superclass
