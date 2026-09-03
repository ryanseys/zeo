p Thread.instance_methods(false).include?(:==)
p Fiber.instance_methods(false).include?(:equal?)
p Encoding.instance_methods(false).include?(:hash)
p Module.instance_methods(false).include?(:instance_variable_get)
p Module.instance_method(:instance_variable_get).owner
__END__
false
false
false
false
Kernel
