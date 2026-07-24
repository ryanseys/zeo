# A module class method that resets a module-level hash ivar back to
# `{}` used to emit the default `sp_StrIntHash_new()` regardless of
# the slot's actual variant. When the slot was promoted to e.g.
# sym_poly_hash by other writes, the reset path mismatched the
# const's declared type at C compile.
#
# Two failures in one shape:
#   1. Wrong constructor — empty_hash_coerce' s container-typed `_new()`
#      logic wasn't consulted for module class methods because
#      @current_class_idx was -1 (modules don't enter the class scope).
#   2. Wrong LHS — `self->iv_X` for a module class method is dangling
#      (no self). ivar_lhs already routes `@X` inside `Mod_cls_*` to
#      `cst_<Mod>_<X>`; the codegen path was hardcoded to
#      `self->` and bypassed it.

module Counters
  @counts = {}

  def self.add(name, n)
    @counts[name] = (@counts[name] || 0) + n
  end

  def self.fetch(name)
    @counts[name] || 0
  end

  def self.reset!
    @counts = {}
  end
end

Counters.add(:apples, 3)
Counters.add(:oranges, 5)
puts Counters.fetch(:apples)
puts Counters.fetch(:oranges)
Counters.reset!
puts Counters.fetch(:apples)
Counters.add(:apples, 7)
puts Counters.fetch(:apples)

# Array variant — same shape: module-level @list = [] in reset.
module Events
  @list = []

  def self.record(s)
    @list << s
  end

  def self.first
    @list[0] || ""
  end

  def self.reset!
    @list = []
  end
end

Events.record("one")
Events.record("two")
puts Events.first
Events.reset!
puts Events.first
Events.record("after_reset")
puts Events.first
