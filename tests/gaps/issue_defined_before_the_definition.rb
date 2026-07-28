# `defined?(Const)` asked BEFORE the constant's definition runs answers
# "constant" under zeo and nil under ruby: the class is registered at compile
# time, so `resolve_class` finds it wherever in the program it is written, while
# ruby only knows about it once the `class` statement has executed.
#
# Both the emission-time fold (`codegen::constfold::const_form_resolves`) and
# the registration-time one (`guard_fold::defined_const_fold`) answer from the
# same whole-program view, so they agree with each other and disagree with ruby.
# Deciding this properly needs document-order reachability, not just "is it
# defined somewhere".
p defined?(Later)
class Later; end
p defined?(Later)

module Wrapper
  p defined?(Inner)
  class Inner; end
  p defined?(Inner)
end

# The same question through a conditional, which is the shape that matters --
# a compat shim guarding on a constant its own file defines further down.
if defined?(Sentinel)
  p :early
else
  p :not_yet
end
Sentinel = 1
p defined?(Sentinel)

# The mirror image, and a separate cause: a TOP-LEVEL value constant answers nil
# even AFTER its assignment. `mro::directly_defines_const` reads a class's
# `class_body_stmts`, and a top-level `NAME = ...` is in the main statement
# stream instead, belonging to no class body -- so nothing records it against
# `Object`, where ruby puts it. (Inside a class or module body it resolves; see
# `tests/const_probe_and_anchored_shell.rb`.)
AT_TOP_LEVEL = 7
p defined?(AT_TOP_LEVEL)
