# A class-body `extend M` runs where it is WRITTEN, not at class registration.
#
# `extend M` is a compile-time ancestry edit, so it used to be applied before
# the body's first statement. Ruby applies it in document order, and the
# difference is observable the moment the same body also edits the singleton
# chain at run time: `rb_include_module` searches the WHOLE chain
# (`search_super = TRUE`) and `rb_prepend_module` only the PREPEND AREA, so
# whichever verb runs FIRST is the one that finds an empty scope. `prepend M`
# then `extend M` gives the module one singleton position, because the extend
# finds the prepended copy; the other order gives it two.
#
# The fix is the shape [[zeo-positional-runtime-flags]] keeps finding, and it
# needed no new mechanism. Both RUNTIME spellings were already right in either
# order, and `defer_runtime_mixin_in_body` already turns a class-body mixin
# into a send at its own position -- for a module that defines methods
# dynamically, and for one that overrides `extend_object`. A body that edits
# its own singleton chain at run time is a third reason, and that is the whole
# change.
#
# The narrowing matters: only a body that performs a run-time singleton edit
# defers. Every other class-body `extend` stays a compile-time edit, so the
# static singleton chain and class-method dispatch are untouched.
#
# The sweep found three shapes past the two below -- an `extend` of a DIFFERENT
# module after a prepend, two prepends against two extends, and an `include`
# between them -- and they are in
# tests/a_singleton_chain_edit_orders_against_a_class_body_extend.rb.

module M
  def hi = "m"
end

class Reversed
  singleton_class.prepend M
  extend M
end
p Reversed.singleton_class.ancestors.map(&:to_s).first(3)

# The same two verbs written the other way round agree, and so do both
# runtime spellings.
class Ordered
  extend M
  singleton_class.prepend M
end
p Ordered.singleton_class.ancestors.map(&:to_s).first(4)
