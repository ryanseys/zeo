# The rows `Struct` reports beside its own.
p Struct.private_instance_methods(false).sort
S = Struct.new(:a)
p S.new(1).respond_to?(:__zeo_struct_init, true)
p Struct.private_method_defined?(:__zeo_struct_init)
__END__
[:initialize, :initialize_copy]
false
false
