# Issue #713. Module class methods (`def self.X`) writing/reading
# `@ivar` need a module-level `cst_<Mod>_<ivar>` slot or the codegen
# emits `self->iv_X` against a non-existent `self` in the top-level
# function. The walk now scans each `def self.X` body and pre-registers
# every observed @ivar as a const slot.

module M
  def self.set(v); @v = v; end
  def self.get; @v; end
end
M.set(42)
puts M.get
M.set(100)
puts M.get
