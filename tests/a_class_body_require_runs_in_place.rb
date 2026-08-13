# A `require` written in a `class`/`module` BODY runs AT ITS OWN POSITION in
# CRuby, at the top-level cref -- not after the whole enclosing file.
#
# zeo used to append the target's statements to the file-trailing splice, so
# `module M; puts "before"; require_relative "x"; puts "after"; p X; end`
# printed before/after/inner and then raised `uninitialized constant M::X`:
# wrong order, and the constant landed in the wrong namespace on the way.
#
# The target is now compiled as its own FEATURE UNIT -- a free function at the
# top-level cref -- and the CALL stays live, so the runtime `require` resolves
# and runs it in place. The compile-time PRE-LOWER still happens first, which
# is what keeps a required FFI vocabulary visible to the statements below it
# (see `a_module_body_require_declares_before_the_next_statement`).
module M
  puts "before"
  require_relative "a_class_body_require_runs_in_place/inner"
  puts "after"
  p INNER_TOP
  p InnerClass.hi
end

# The constant went to Object, not to M.
p M.constants
p Object.const_defined?(:INNER_TOP)
p defined?(M::INNER_TOP)

# A second require of the same file is a no-op, and answers false.
module N
  p require_relative("a_class_body_require_runs_in_place/inner")
end
puts "done"
