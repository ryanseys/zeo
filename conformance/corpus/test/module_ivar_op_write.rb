# Module-level @ivar referenced inside `def self.X` (the canonical
# Ruby singleton-state pattern) used to emit as `self->iv_X` in C.
# The class method has no self parameter -- module class methods
# dispatch through the module's static table rather than an
# instance pointer -- so the use site failed to compile.
#
# Fix: `ivar_lhs` now detects the "current emit scope is a module
# class method" case and routes the lhs / rhs to the module's
# `cst_<Mod>_<name>` static slot. The InstanceVariableReadNode
# arm already had this dispatch inline; the operator-write / or-
# write / and-write arms reached `ivar_lhs` and got the wrong
# `self->iv_X` expansion.
#
# Coverage:
#   - Plain `@count += 1` (InstanceVariableOperatorWriteNode).
#   - `@count = N` (InstanceVariableWriteNode).
#   - `@flag ||= true` (InstanceVariableOrWriteNode).
#   - Read access via `@count` (InstanceVariableReadNode) --
#     unchanged but exercised by the puts below.

module Counter
  @count = 0
  @flag = false

  def self.increment
    @count += 1
  end

  def self.reset
    @count = 0
  end

  def self.set_flag
    @flag ||= true
  end

  def self.value
    @count
  end

  def self.flag?
    @flag
  end
end

Counter.increment
Counter.increment
Counter.increment
puts Counter.value
Counter.set_flag
puts Counter.flag?
Counter.reset
puts Counter.value
