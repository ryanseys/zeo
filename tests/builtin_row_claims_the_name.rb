# A NATIVE row on a class claims its name in the MRO exactly as a `def` does.
# `Array#none?` and `Array#size` are Rust builtins with no user scope, so
# materialization used to fall through to the first ancestor that DID have one
# -- handing a `module Enumerable` reopen a name `Array` owns. Ruby puts
# `Array` first in the ancestry and its own definition wins.

# A user REOPEN of the builtin outranks the native row it replaces. Written
# up top so the whole file reads one answer -- when the reopen wins is the
# separate question `a_later_def_on_a_builtin_reaches_back` pins.
class Array
  def take_while; :array_own; end
end

module Enumerable
  def take_while; :enum_take_while; end
  def none?; :enum_none; end
  def size; :enum_size; end
  def zzz_only_here; :enum_only; end
end

# Array owns both natively, so the module reopen loses.
p [1].take_while { true }
p [1].none?
p [1].size
p [].none? { |x| x > 9 }
# ... but a name only the module has still reaches.
p [1].zzz_only_here

# Every other Enumerable host answers from its own native row too.
p({ a: 1 }.size)
p((1..3).size)
S0 = Struct.new(:a)
p S0.new(1).size

# The claim is per-ancestor, so a subclass inherits the winner.
class MyArr < Array; end
p MyArr.new.none?
p MyArr.new.size

module M1
  def size; :m1; end
end

# `prepend` sits AHEAD of the class, so it beats the native row.
class WithPrepend < Array
  prepend M1
end
p WithPrepend.new.size

# `include` in a SUBCLASS also sits ahead of the parent's native row.
class WithInclude < Array
  include M1
end
p WithInclude.new.size

# `include` in the class ITSELF sits behind its own rows.
class Array
  include M1
end
p [1, 2].size

# A name the host does NOT own natively still comes from the module.
module Comparable
  def between?(a, b); :cmp_between; end
end
p 5.between?(1, 9)
p 5.clamp(1, 3)

module Kernel
  def frozen?; :kernel_frozen; end
end
p "x".frozen?
p Object.new.frozen?

