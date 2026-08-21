# A module PREPENDED and INCLUDED on the same class appears TWICE in
# CRuby's ancestry -- once ahead of the class, once behind it. zeo lists it
# once, in the prepend's position.
#
# The reason is structural, and it is why this is a gap rather than a bug
# with a one-line fix: zeo's ancestry is a `Vec<ClassId>`, a flat leaked
# snapshot, and every walk over it positions by the FIRST match --
# `send_super_from` resumes after `position(|&a| a == defining)`,
# `class_method_owner_after` likewise, and the emitted `ClassDesc.ancestors`
# is a `u32` array with the same shape. CRuby can hold the module twice
# because each occurrence is a distinct ICLASS object with its own identity;
# zeo has nothing to tell the two apart with, so a duplicate entry would
# make a `super` from the INCLUDED copy resume after the PREPENDED one.
#
# The fix shape: the ancestry element becomes a `(ClassId, u32 occurrence)`
# pair (or the mixin gets a per-splice surrogate id, which is what
# `REG_SINGLETON_SURROGATE` already does for a singleton), and the ~10 walks
# that position by class id take the occurrence with it. That is an ABI
# change (`ClassDesc.ancestors`) plus every dispatch walk, so it belongs
# with a deliberate MRO pass rather than beside a divergence sweep.

module MX
  def hi = "mx(#{defined?(super) ? super : 'top'})"
end

class Both
  include MX
  prepend MX
  def hi = "Both"
end

p Both.ancestors.map(&:to_s)
p Both.new.hi

class Runtime
  def hi = "Runtime"
end
Runtime.include MX
Runtime.prepend MX
p Runtime.ancestors.map(&:to_s)
