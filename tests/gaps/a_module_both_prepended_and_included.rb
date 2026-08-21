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
# THE EXACT CRUBY RULE, measured 2026-08-21 by the combinatorial sweep in
# `tests/mixins_across_classes_modules_and_singletons.rb`. It is asymmetric,
# and the asymmetry is the whole of the behaviour:
#
#   include A; prepend A   ->  [A, K, A]   the module appears TWICE
#   prepend A; include A   ->  [A, K]      the include is a no-op
#   include A; include A   ->  [K, A]
#   prepend A; prepend A   ->  [A, K]
#
# `rb_include_module` calls `include_modules_at(klass, ORIGIN(klass), module,
# search_super = TRUE)`, so `include` searches the WHOLE chain and skips a
# module already anywhere in it. `rb_prepend_module` passes
# `search_super = FALSE`, so `prepend` searches only the prepend area and adds
# a module that is merely included below. DOCUMENT ORDER therefore decides:
# whichever verb runs first is the one that finds an empty chain.
#
# zeo cannot reproduce that today for a second reason, beyond the flat
# `Vec<ClassId>`: `ClassInfo` keeps `includes` and `prepends` as two separate
# lists, so the order the two verbs ran in is not recorded at all.
#
# The fix shape, in three parts:
#
#   1. `ClassInfo` records the mixin operations in DOCUMENT ORDER (one
#      `Vec<(Placement, ClassId)>` beside the two existing lists, which stay
#      for their readers).
#   2. `mro::expand_into` replays them the way `include_modules_at` does,
#      rather than expanding two lists with one global dedup.
#   3. The ancestry element gains an OCCURRENCE, so `super` from the included
#      copy resumes after IT and not after the prepended one. A per-splice
#      surrogate id is the shape that already exists in the tree --
#      `REG_SINGLETON_SURROGATE` mints exactly this kind of stand-in for a
#      singleton class, and `ancestors`/`include?`/`is_a?` map it back to the
#      module it names. Seven `position(|&a| a == ..)` walks in
#      `dispatch/` + `runtime_meta/` take the occurrence with them.
#
# Part 3 is why this is a deliberate MRO pass and not a divergence-sweep fix.

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
