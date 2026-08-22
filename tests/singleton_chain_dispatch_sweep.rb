# The CLASS-METHOD channel swept against ruby: every way a module can be
# seated in a singleton class, every order they can be written in, and what
# `super`, `defined?(super)`, `Method#owner`, `#super_method` and the
# reflection surface answer at each position.
#
# zeo has no singleton-class OBJECTS, so `#<Class:K>` is derived from the
# instance ancestry: per non-module ancestor, its singleton prepends, its own
# `def self.x` layer, then its extends. A class method resolves down that
# derived chain exactly as an instance method resolves down an ordinary one --
# which is why this file asks the same questions of both sides wherever it
# can.
#
# The shapes that made it worth sweeping: a module reached BOTH ways holds two
# positions and runs once per position; a materialized copy of an extended
# module's method bakes the class it was extended ONTO while sitting at the
# MODULE's position; and a `super` resolved at compile time enters a body the
# walk also knows a position for, so it has to publish the same one.

puts "== one module, one position, super reaches the tail"
module M1
  def tag = "M1(#{defined?(super) ? super : :top})"
end
class One
  extend M1
end
p One.tag
p One.singleton_class.ancestors.map(&:to_s).first(3)
p One.method(:tag).owner.to_s
p [One.respond_to?(:tag), One.singleton_methods(false)]

puts "== an own def self.x sits BEFORE the extended module"
class OwnFirst
  extend M1
  def self.tag = "own(#{defined?(super) ? super : :top})"
end
p OwnFirst.tag
p OwnFirst.method(:tag).owner.to_s
p OwnFirst.method(:tag).super_method&.owner.to_s

puts "== extend order: the LAST extend is closest"
module M2
  def tag = "M2(#{defined?(super) ? super : :top})"
end
class TwoMods
  extend M1
  extend M2
end
p TwoMods.tag
p TwoMods.singleton_class.ancestors.map(&:to_s).first(4)

puts "== a module reached BOTH ways holds two positions"
module CM
  def tag = "cm(#{defined?(super) ? super : :top})"
end
class BothWays
  extend CM
  singleton_class.prepend CM
  def self.tag = "own(#{defined?(super) ? super : :top})"
end
p BothWays.tag
p BothWays.singleton_class.ancestors.map(&:to_s).first(4)

class BothNoOwn
  singleton_class.include CM
  singleton_class.prepend CM
end
p BothNoOwn.tag
p BothNoOwn.singleton_class.ancestors.map(&:to_s).first(4)

puts "== the reverse order gives ONE position"
class PrependFirst; end
PrependFirst.singleton_class.prepend CM
PrependFirst.extend CM
p PrependFirst.tag
p PrependFirst.singleton_class.ancestors.map(&:to_s).first(3)

puts "== two prepends stack, latest closest, and super walks down"
module PA
  def tag = "PA(#{defined?(super) ? super : :top})"
end
module PB
  def tag = "PB(#{defined?(super) ? super : :top})"
end
class Stacked
  def self.tag = "own(#{defined?(super) ? super : :top})"
end
Stacked.singleton_class.prepend PA
Stacked.singleton_class.prepend PB
p Stacked.tag
p Stacked.singleton_class.ancestors.map(&:to_s).first(4)

puts "== a module that includes another, extended"
module Inner
  def tag = "inner(#{defined?(super) ? super : :top})"
end
module Outer
  include Inner
  def tag = "outer(#{super})"
end
class Nested
  extend Outer
end
p Nested.tag
p Nested.singleton_class.ancestors.map(&:to_s).first(4)
p Nested.method(:tag).owner.to_s

puts "== inheritance: the parent's extend is reached from the child"
class Parent
  extend M1
end
class Child < Parent; end
class GrandChild < Child; end
p [Parent.tag, Child.tag, GrandChild.tag]
p Child.singleton_class.ancestors.map(&:to_s).first(4)
p [Parent.method(:tag).owner.to_s, GrandChild.method(:tag).owner.to_s]

puts "== the child adds its own extend, which sits ahead of the parent's"
class ChildExtends < Parent
  extend M2
end
p ChildExtends.tag
p ChildExtends.singleton_class.ancestors.map(&:to_s).first(5)

puts "== a prepend on the PARENT's singleton, reached by super from the child"
class PrependedParent
  def self.tag = "pp(#{defined?(super) ? super : :top})"
end
PrependedParent.singleton_class.prepend PA
class PrependedChild < PrependedParent
  def self.tag = "pc(#{defined?(super) ? super : :top})"
end
p [PrependedParent.tag, PrependedChild.tag]
p PrependedChild.singleton_class.ancestors.map(&:to_s).first(5)

puts "== super reaches the instance tail (Class's own methods)"
module Tagger
  def to_s = "T(#{super})"
  def name = "N(#{super})"
end
class Tailed
  extend Tagger
end
p Tailed.to_s
p Tailed.name
p Tailed.inspect

puts "== super with explicit arguments, and zsuper, through a mixin"
module ArgMod
  def build(a, b = 2) = "argmod(#{a},#{b})"
end
class ArgBase
  extend ArgMod
end
module ArgOver
  def build(a, b = 2) = "over(#{super(a, b * 10)})"
end
class ArgHost < ArgBase
  extend ArgOver
  def self.build(a, b = 2) = "own(#{super})"
end
p ArgHost.build(1)
p ArgHost.build(1, 3)

puts "== define_singleton_method beats a prepended module's copy? no -- prepend wins"
class DynHost
  def self.tag = "own(#{defined?(super) ? super : :top})"
end
DynHost.singleton_class.prepend PA
DynHost.define_singleton_method(:tag) { "dyn(#{:noSuper})" }
p DynHost.tag

puts "== a runtime class takes the same rules"
RtBase = Class.new do
  def self.tag = "rt_own(#{defined?(super) ? super : :top})"
end
RtBase.extend M1
p RtBase.tag
RtSub = Class.new(RtBase)
p RtSub.tag

puts "== a MODULE's own singleton chain"
module HasSingleton
  extend M1
  def self.tag = "modown(#{defined?(super) ? super : :top})"
end
p HasSingleton.tag
p HasSingleton.singleton_class.ancestors.map(&:to_s).first(3)

puts "== remove_method in class << self empties one position, undef ends the walk"
class Removed
  extend M1
  def self.tag = "own(#{defined?(super) ? super : :top})"
end
class Removed
  class << self
    remove_method :tag
  end
end
p Removed.tag
class Undefed
  extend M1
  def self.tag = "own"
end
class Undefed
  class << self
    undef_method :tag
  end
end
begin
  Undefed.tag
rescue NoMethodError
  p "undef ends the walk"
end

puts "== a class-level method_missing is reached through the same chain"
module MMod
  def method_missing(n, *a) = n == :ghost ? "mmod_ghost" : super
  def respond_to_missing?(n, p = false) = n == :ghost || super
end
class Ghosted
  extend MMod
end
p Ghosted.ghost
p Ghosted.respond_to?(:ghost)

puts "== instance and class channels answer the same shape independently"
module Dual
  def tag = "dual(#{defined?(super) ? super : :top})"
end
class BothChannels
  include Dual
  extend Dual
  def tag = "inst(#{super})"
  def self.tag = "cls(#{super})"
end
p [BothChannels.new.tag, BothChannels.tag]
p BothChannels.ancestors.first(3).map(&:to_s)
p BothChannels.singleton_class.ancestors.map(&:to_s).first(3)

puts "== #super_method walks the singleton chain, one position per step"
module S1
  def tag = "S1"
end
module S2
  def tag = "S2(#{super})"
end
class Chained
  extend S1
  extend S2
  def self.tag = "own(#{super})"
end
p Chained.tag
m = Chained.method(:tag)
names = []
while m
  names << [m.owner.to_s, m.call]
  m = m.super_method
end
p names

puts "== #super_method through a DOUBLED module stops instead of looping"
class DoubledSeat
  extend CM
  singleton_class.prepend CM
end
d = DoubledSeat.method(:tag)
seats = []
6.times do
  break unless d
  seats << d.owner.to_s
  d = d.super_method
end
p seats

puts "== unbind/bind and instance_method through a singleton class"
p Chained.singleton_class.instance_method(:tag).owner.to_s
p Chained.method(:tag).unbind.owner.to_s
p Chained.singleton_class.ancestors.map(&:to_s).first(4)

puts "== respond_to? and method_defined? agree with the walk"
p [Chained.respond_to?(:tag), Chained.singleton_class.method_defined?(:tag)]
p [Removed.respond_to?(:tag), Undefed.respond_to?(:tag)]

puts "== a frozen class refuses both singleton verbs"
class FrozenSingleton; end
FrozenSingleton.freeze
begin; FrozenSingleton.extend M1; rescue => e; p [e.class, e.message]; end
begin; FrozenSingleton.singleton_class.prepend M1; rescue => e; p e.class; end

puts "== extend on a plain OBJECT keeps its own chain"
obj = Object.new
module ObjMod
  def tag = "obj(#{defined?(super) ? super : :top})"
end
obj.extend ObjMod
p obj.tag
p obj.singleton_class.ancestors.map(&:to_s).first(2)
p obj.method(:tag).owner.to_s

puts "== a module extended onto a MODULE, and its super"
module Extendee
  extend M1
  def self.tag = "modself(#{defined?(super) ? super : :top})"
end
p Extendee.tag
p Extendee.method(:tag).owner.to_s
p Extendee.method(:tag).super_method&.owner.to_s

puts "== the walk is stable across repeated asks"
3.times { print Chained.tag, " " }
puts
p Chained.singleton_class.ancestors.map(&:to_s).first(4)
