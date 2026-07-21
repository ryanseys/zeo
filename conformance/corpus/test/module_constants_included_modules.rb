# Module#include? / #included_modules / #constants (#2674). The constant
# registry is a flat namespace, so ownership is recovered from the AST: a
# class's own constants are the ConstantWriteNodes in its body.
#
# NOTE: the order of constants WITHIN one class is not specified -- CRuby reads
# them out of an id table, so it varies between programs. Only the grouping is
# meaningful (own first, then the other ancestors), so multi-constant classes
# are compared sorted.
module Walks; end
class Animal; end
class Dog < Animal; include Walks; end
p Dog.include?(Walks)
p Dog.included_modules
p Dog.included_modules.include?(Walks)
p Dog.constants

module Mod; MC = 9; MD = 10; end
class A; X = 1; end
class B < A; Y = 2; include Mod; end
p A.constants
p B.constants.sort
p B.constants(false)
p Mod.constants.sort
p B.constants.map { |s| s.to_s }.sort

# own comes before the inherited groups, and the groups follow #ancestors
module P; PC = 5; end
module I; IC = 7; end
class Own1; prepend P; include I; Q = 6; end
p Own1.ancestors
p Own1.constants
class A2; AC = 1; end
class B2 < A2; prepend P; BC = 2; end
p B2.constants

class Empty; end
p Empty.constants
module M2; end
p M2.constants
