# A real, previously-undetected bug, found alongside the `||=` fix
# above: the compiler's call lowering treated ANY `HirNode::ClassRef`
# receiver as a `ClassName.foo(...)` class-method call, UNCONDITIONALLY
# -- but `ClassRef` is also how an ordinary bare constant read lowers
# (see its own docs). So `MAX += 1` (desugared to `MAX.+(1)`, where
# `MAX` is a plain `Integer` constant, not a class) panicked with a
# confusing "unknown class/module `MAX`" instead of reading/writing
# its actual value -- EVERY compound-assignment operator except `||=`
# on any non-class constant was completely broken. Fixed by only
# taking the class-method-call branch when the name is ACTUALLY a
# registered class/module.

MAX = 100
MAX += 1
puts MAX
MAX -= 50
puts MAX
__END__
101
51
#@ stderr
lang/variables/compound_assignment_on_a_plain_non_class_constant_dispatches_correctly.rb:14: warning: already initialized constant MAX
lang/variables/compound_assignment_on_a_plain_non_class_constant_dispatches_correctly.rb:13: warning: previous definition of MAX was here
lang/variables/compound_assignment_on_a_plain_non_class_constant_dispatches_correctly.rb:16: warning: already initialized constant MAX
lang/variables/compound_assignment_on_a_plain_non_class_constant_dispatches_correctly.rb:14: warning: previous definition of MAX was here
