# The mix-in surface, combinatorially: `include`/`prepend`/`extend` against
# classes, modules, singleton classes and plain objects, read off `ancestors`,
# `super` chains, the notification hooks and the reflection predicates.
#
# Written 2026-08-21 as a differential sweep against ruby 4.0.6. It found four
# divergences on its first run, three of them fixed in the same commit series:
# a singleton `prepend` fired no `prepended` hook, its module landed AFTER the
# singleton head in `ancestors` instead of before, and
# `singleton_class.include M` reported itself as `M.extended(K)` rather than
# `M.included(#<Class:K>)`. The fourth is `tests/gaps/
# a_module_both_prepended_and_included.rb`, which is why `include A; prepend A`
# is not among the shapes below.

module A; end
module B; end

puts "-- ancestry, one class, every ordering"
class I1;  include A;  include B;  end
class I2;  include A, B;           end
class P1;  prepend A;  prepend B;  end
class P2;  prepend A, B;           end
class M1;  include A;  prepend B;  end
class M2;  prepend B;  include A;  end
class M4;  prepend A;  include A;  end   # the include is a no-op: A is already there
class M5;  include A;  include A;  end
class M6;  prepend A;  prepend A;  end
[I1, I2, P1, P2, M1, M2, M4, M5, M6].each do |k|
  puts "#{k}: #{k.ancestors.take_while { |a| a != Object }.map(&:to_s).join(' ')}"
end

puts "-- a module that mixes in a module, then joins a class"
module Inner; end
module Outer;  include Inner; end
module POuter; prepend Inner; end
class Host;  include Outer;  end
class PHost; include POuter; end
p Host.ancestors.take_while { |a| a != Object }.map(&:to_s)
p PHost.ancestors.take_while { |a| a != Object }.map(&:to_s)

puts "-- super through prepend / class / include / superclass"
module Pre;  def who = "Pre(#{super})";  end
module Inc;  def who = "Inc(#{super})";  end
module Inc2; def who = "Inc2(#{super})"; end
class SBase; def who = "SBase";          end
class SKid < SBase
  prepend Pre
  include Inc
  include Inc2
  def who = "SKid(#{super})"
end
p SKid.new.who
p SKid.ancestors.take_while { |a| a != Object }.map(&:to_s)

module P1x; def tag = "P1(#{super})"; end
module P2x; def tag = "P2(#{super})"; end
class T; def tag = "T"; prepend P1x; prepend P2x; end
p T.new.tag

puts "-- a prepend reaching super into a module included on the SUPERCLASS"
module GrandInc; def deep = "GrandInc"; end
class GBase; include GrandInc; end
module GPre; def deep = "GPre(#{super})"; end
class GKid < GBase; prepend GPre; end
p GKid.new.deep

puts "-- the singleton side, every spelling"
module SA; def s = "SA(#{super rescue 'x'})"; end
module SB; def s = "SB(#{super rescue 'x'})"; end
class One;  def self.s = "One";  class << self; prepend SA; end; end
class Two;  def self.s = "Two";  singleton_class.prepend SA;    end
class Four; def self.s = "Four"; extend SA;                     end
p [One.s, Two.s, Four.s]
p Two.singleton_class.ancestors.map(&:to_s).take(3)
p Four.singleton_class.ancestors.map(&:to_s).take(3)

class Five; def self.s = "Five"; end
Five.singleton_class.prepend SA
Five.singleton_class.prepend SB
p Five.s, Five.singleton_class.ancestors.map(&:to_s).take(3)

class Six; def self.s = "Six"; end
Six.extend SA
Six.extend SB
p Six.s, Six.singleton_class.ancestors.map(&:to_s).take(3)

puts "-- an object's own singleton"
module OA; def hi = "OA(#{super rescue 'x'})"; end
class Plain; def hi = "Plain"; end
o = Plain.new
o.extend OA
p o.hi
p o.singleton_class.ancestors.map(&:to_s).drop(1).take(2)
p [o.is_a?(OA), Plain.new.is_a?(OA), o.singleton_class.include?(OA)]

puts "-- every hook, and the argument each receives"
module H
  def self.included(b)  = puts("included #{b}")
  def self.prepended(b) = puts("prepended #{b}")
  def self.extended(b)  = puts("extended #{b}")
end
class C1; include H; end
class C2; prepend H; end
class C3; extend  H; end
class C4; end
C4.include H
C4.prepend H
C4.extend  H
C4.singleton_class.prepend H
C4.singleton_class.include H
module MH; include H; end

puts "-- multi-arg include notifies in reverse"
module H2; def self.included(b) = puts("H2 included #{b}"); end
module H3; def self.included(b) = puts("H3 included #{b}"); end
class C5; include H2, H3; end

puts "-- the three primitives run, and their hooks follow"
module Prim
  def self.append_features(b)  = (puts("append_features #{b}"); super)
  def self.prepend_features(b) = (puts("prepend_features #{b}"); super)
  def self.extend_object(b)    = (puts("extend_object #{b}"); super)
  def self.included(b)  = puts("Prim included #{b}")
  def self.prepended(b) = puts("Prim prepended #{b}")
  def self.extended(b)  = puts("Prim extended #{b}")
  def pm = :pm
end
class C6; include Prim; end
class C7; prepend Prim; end
class C8; extend  Prim; end
p [C6.new.pm, C7.new.pm, C8.pm]

puts "-- reflection agrees with the ancestry"
module R1; def r = 1; end
module R2; def r = 2; end
class RC; include R1; prepend R2; def r = 0; end
p [RC.include?(R1), RC.include?(R2)]
p RC.included_modules.take(3).map(&:to_s)
p [RC.instance_method(:r).owner.to_s, RC.new.method(:r).owner.to_s]
p [RC.method_defined?(:r), RC.instance_methods(false)]
p [RC < R1, RC < R2, R1 <=> RC, RC <=> R1]
p [RC.new.is_a?(R1), RC.new.is_a?(R2), R1 === RC.new]

puts "-- runtime-minted classes take the same rules"
RM = Module.new { def q = :rm }
RK = Class.new  { def q = :rk }
RK.prepend RM
p [RK.new.q, RK.ancestors.first.equal?(RM)]
RK2 = Class.new { def q = :rk2 }
RK2.include RM
p RK2.new.q

puts "-- a LATER include on a module reaches the class already hosting it"
module Late; end
module Mid;  end
class LateHost; include Mid; end
module Mid; include Late; end
p LateHost.ancestors.take_while { |a| a != Object }.map(&:to_s)
p LateHost.new.is_a?(Late)

puts "-- a frozen target refuses both verbs"
class FrozenTarget; end
FrozenTarget.freeze
begin; FrozenTarget.include R1; rescue => e; p [e.class, e.message]; end
begin; FrozenTarget.prepend R1; rescue => e; p [e.class, e.message]; end

puts "-- a module reached both ways holds TWO singleton positions, and order decides"
# `rb_include_module` searches the whole chain and `rb_prepend_module` only the
# prepend area, so whichever verb runs FIRST is the one that finds an empty
# scope. `extend` then `prepend` gives two positions; the reverse gives one.
module SP
  def sp = "sp"
end
class Both1
  extend SP
  singleton_class.prepend SP
end
p [Both1.singleton_class.ancestors.map(&:to_s).first(4), Both1.is_a?(SP)]

class Both2
  singleton_class.include SP
  singleton_class.prepend SP
end
p [Both2.singleton_class.ancestors.map(&:to_s).first(4), Both2.is_a?(SP)]

class Rev1; end
Rev1.singleton_class.prepend SP
Rev1.singleton_class.include SP
p Rev1.singleton_class.ancestors.map(&:to_s).first(3)

class Rev2; end
Rev2.singleton_class.prepend SP
Rev2.extend SP
p Rev2.singleton_class.ancestors.map(&:to_s).first(3)

class Twice; end
Twice.singleton_class.prepend SP
Twice.singleton_class.prepend SP
p Twice.singleton_class.ancestors.map(&:to_s).first(3)
__END__
-- ancestry, one class, every ordering
I1: I1 B A
I2: I2 A B
P1: B A P1
P2: A B P2
M1: B M1 A
M2: B M2 A
M4: A M4
M5: M5 A
M6: A M6
-- a module that mixes in a module, then joins a class
["Host", "Outer", "Inner"]
["PHost", "Inner", "POuter"]
-- super through prepend / class / include / superclass
"Pre(SKid(Inc2(Inc(SBase))))"
["Pre", "SKid", "Inc2", "Inc", "SBase"]
"P2(P1(T))"
-- a prepend reaching super into a module included on the SUPERCLASS
"GPre(GrandInc)"
-- the singleton side, every spelling
["SA(One)", "SA(Two)", "Four"]
["SA", "#<Class:Two>", "#<Class:Object>"]
["#<Class:Four>", "SA", "#<Class:Object>"]
"SB(SA(Five))"
["SB", "SA", "#<Class:Five>"]
"Six"
["#<Class:Six>", "SB", "SA"]
-- an object's own singleton
"OA(Plain)"
["OA", "Plain"]
[true, false, true]
-- every hook, and the argument each receives
included C1
prepended C2
extended C3
included C4
prepended C4
extended C4
prepended #<Class:C4>
included #<Class:C4>
included MH
-- multi-arg include notifies in reverse
H3 included C5
H2 included C5
-- the three primitives run, and their hooks follow
append_features C6
Prim included C6
prepend_features C7
Prim prepended C7
extend_object C8
Prim extended C8
[:pm, :pm, :pm]
-- reflection agrees with the ancestry
[true, true]
["R2", "R1", "Kernel"]
["R2", "R2"]
[true, [:r]]
[true, true, 1, -1]
[true, true, true]
-- runtime-minted classes take the same rules
[:rm, true]
:rk2
-- a LATER include on a module reaches the class already hosting it
["LateHost", "Mid", "Late"]
true
-- a frozen target refuses both verbs
[FrozenError, "can't modify frozen Class: FrozenTarget"]
[FrozenError, "can't modify frozen Class: FrozenTarget"]
-- a module reached both ways holds TWO singleton positions, and order decides
[["SP", "#<Class:Both1>", "SP", "#<Class:Object>"], true]
[["SP", "#<Class:Both2>", "SP", "#<Class:Object>"], true]
["SP", "#<Class:Rev1>", "#<Class:Object>"]
["SP", "#<Class:Rev2>", "#<Class:Object>"]
["SP", "#<Class:Twice>", "#<Class:Object>"]
