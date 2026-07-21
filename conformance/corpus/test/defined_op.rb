# Issue #711. `defined?(expr)` used to emit a raw pointer value
# (printed as a giant integer) instead of a tag string. Now resolves
# at compile time to "local-variable" / "instance-variable" /
# "constant" / "method" / "expression" with a NULL `const char*`
# for the nil cases (which `puts` already handles as an empty line).

puts defined?(undefined_local)
z = 1
puts defined?(z)
puts defined?(@undef_ivar)
puts defined?(Object)
puts defined?(undefined_method)
puts defined?(nil)
puts defined?(self)

@written_ivar = "x"
puts defined?(@written_ivar)
