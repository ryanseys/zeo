# ObjectSpace's introspection half comes from `require "objspace"`, and
# `Kernel#BigDecimal` from `require "bigdecimal"`.
p ObjectSpace.respond_to?(:memsize_of), ObjectSpace.respond_to?(:dump)
p ObjectSpace.respond_to?(:count_objects), ObjectSpace.respond_to?(:each_object)
p Kernel.private_method_defined?(:BigDecimal)
require "objspace"
p ObjectSpace.respond_to?(:memsize_of), ObjectSpace.respond_to?(:dump)
require "bigdecimal"
p Kernel.private_method_defined?(:BigDecimal)
p BigDecimal("1.5").to_s
__END__
false
false
true
true
false
true
true
true
"0.15e1"
