# `class X < Module` -- a module FACTORY, whose instances are real modules.
#
# This is NOT the value-subclass payload shape and must not become it: a
# wrapper holding a module is not a module, so it would fail `include`,
# `Module#===`, constant lookup and `ancestors`. Instead `X.new` mints a real
# runtime module id and tags it as belonging to X, so only the question "what
# is your class?" changes answer.
#
# 22 of the gem corpus's `Module` rows come from ONE file --
# `activesupport/lib/active_support/deprecation/proxy_wrappers.rb` -- which
# gates actioncable, actionmailer, actionview, activejob, activemodel,
# activerecord, activestorage and the rest of the Rails stack.

class LazilyDefineAttributes < Module
  def initialize(attributes)
    @attributes = attributes
  end

  def included(base)
    base.instance_variable_set(:@lazy, @attributes)
  end

  def attributes
    @attributes
  end
end

m = LazilyDefineAttributes.new([:a, :b])

# Its class is the subclass, and it is a module all the same.
p [m.class, m.is_a?(Module), m.is_a?(LazilyDefineAttributes), m.instance_of?(LazilyDefineAttributes)]
p Module === m

# A user instance method runs against it, and ivars persist across calls --
# `initialize` wrote `@attributes`, `attributes` reads it back.
p m.attributes

# `include`ing one runs the user `included` hook, because the value really is
# a module: `Module#include` reaches it unchanged.
class Host
  include LazilyDefineAttributes.new([:x])
end

p Host.instance_variable_get(:@lazy)
p Host.ancestors.include?(Module)

# Reflection on the factory itself.
p LazilyDefineAttributes.superclass
p LazilyDefineAttributes.instance_of?(Class)

# Ivars set from OUTSIDE reach the same store the method bodies use.
m.instance_variable_set(:@extra, 9)
p [m.instance_variable_get(:@extra), m.instance_variables.sort]

# The Rails shape itself: a `self.new` override that can answer a NON-module,
# and a subclass that inherits `initialize`.
class DeprecatedConstantProxy < Module
  def self.new(*args, **options, &block)
    object = args.first
    return object unless object
    super
  end

  def initialize(old_const, new_const)
    @old_const = old_const
    @new_const = new_const
  end

  def inspect = "DeprecatedConstantProxy(#{@old_const} -> #{@new_const})"

  def target = @new_const
end

# `new` returning something that is not an instance of the class at all.
p DeprecatedConstantProxy.new(nil)

pr = DeprecatedConstantProxy.new("OLD", "NEW")
p [pr.class, pr.inspect, pr.target, pr.is_a?(Module)]

class Sub < DeprecatedConstantProxy
end

s = Sub.new("A", "B")
p [s.class, s.target, s.is_a?(DeprecatedConstantProxy), s.is_a?(Module)]

# A factory that defines no `initialize` at all takes no arguments -- `Module`'s
# own must not run in its place.
class Bare < Module
end

b = Bare.new
p [b.class, b.is_a?(Module)]

# `ObjectSpace::WeakMap` is the third native shape, and the cleanest: its
# constructor already builds `WeakMap::new(class)` from the receiver, so a
# subclass instance IS the native type -- no payload wrapper, no re-tagging.
# activesupport's `DescendantsTracker::WeakSet` is the case, and it gates
# eleven Rails gems on its own.
class WeakSet < ObjectSpace::WeakMap
  alias_method :to_a, :keys

  def <<(object)
    self[object] = true
    self
  end
end

ws = WeakSet.new
p [ws.class, ws.is_a?(ObjectSpace::WeakMap), WeakSet.superclass]

one = "one"
two = "two"
ws << one
ws << two
p [ws.to_a.map(&:itself).sort, ws.size, ws[one]]
