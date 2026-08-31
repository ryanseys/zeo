require_relative "a_def_after_an_undef_in_a_unit/tracer"
p Module.private_method_defined?(:method_added)
p Module.private_instance_methods(false).sort.grep(/method_/)
p Module.new.send(:method_added, :anything)
