# A `module_function` instance copy is PRIVATE, so an explicit receiver must
# raise -- but codegen folds an explicit-receiver builtin call straight to the
# table function without consulting visibility, so it answers instead.
#
# The runtime already knows: `respond_to?` says false, `private_method_defined?`
# says true, and `public_send` raises correctly. Only the folded Path 1 call
# skips the check, because the compiler's `Surface` (crates/zeo/build.rs:240,
# `surface_from_spec`) records `instance_methods` and `class_methods` but no
# visibility, so `enforce_visibility` has nothing to read for a builtin row.
#
# Fix shape: give `Surface` a `private_instance_methods` list, filled from
# `method.visibility == Private || method.is_module_function`, and have the
# explicit-receiver builtin path in `codegen/call` consult it the way
# `enforce_visibility` already consults a user method's `scope.visibility`.
#
# Predates the module_function conversion -- `5.puts("x")` has always answered
# where ruby raises, for the same reason.

class Host
  include Process
end

begin
  Host.new.pid
  puts "no raise"
rescue NoMethodError => e
  puts e.message
end

begin
  5.puts("x")
  puts "no raise"
rescue NoMethodError => e
  puts e.message
end
