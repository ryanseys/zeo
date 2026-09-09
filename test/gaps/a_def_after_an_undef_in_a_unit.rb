# A required file undefs `Module#method_added` and then defines it again.
# After that unit runs, ruby reports the new row as private and lists it
# among the private instance methods; zeo answers from the state before the
# redefinition, so `private_method_defined?` is false and the name is
# missing from the list.
require_relative "a_def_after_an_undef_in_a_unit/tracer"
p Module.private_method_defined?(:method_added)
p Module.private_instance_methods(false).sort.grep(/method_/)
p Module.new.send(:method_added, :anything)
__END__
false
[:method_removed, :method_undefined]
nil
