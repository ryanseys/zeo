# Ruby puts `using` on main's singleton and on Module, not on Kernel.
p Kernel.private_method_defined?(:using)
p Kernel.method_defined?(:using)
p Module.private_method_defined?(:using)
p self.singleton_class.private_method_defined?(:using)
__END__
false
false
true
true
