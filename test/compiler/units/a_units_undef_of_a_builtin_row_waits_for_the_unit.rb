if ARGV.include?("--debug")
  require_relative "a_units_undef_of_a_builtin_row_waits_for_the_unit/tracer"
end

p Module.private_method_defined?(:method_added)
p Module.private_instance_methods(false).sort.grep(/method_/)
p Module.new.send(:method_added, :anything)
p BasicObject.private_method_defined?(:singleton_method_added)
__END__
true
[:method_added, :method_removed, :method_undefined]
nil
true
