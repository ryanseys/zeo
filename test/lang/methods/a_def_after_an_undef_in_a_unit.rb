# A required file undefs `Module#method_added` and then defines it again.
# The new row is PUBLIC: it leaves the private list, and `send` reaches it.
require_relative "a_def_after_an_undef_in_a_unit/tracer"
p Module.private_method_defined?(:method_added)
p Module.private_instance_methods(false).sort.grep(/method_/)
p Module.new.send(:method_added, :anything)
__END__
false
[:method_removed, :method_undefined]
nil
