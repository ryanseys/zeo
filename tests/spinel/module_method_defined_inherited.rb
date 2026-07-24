class Animal; def breathe; end; end
class Dog < Animal; def bark; end; end

p Dog.method_defined?(:bark)       # own
p Dog.method_defined?(:breathe)    # user superclass
p Dog.method_defined?(:object_id)  # inherited from Object/Kernel
p Dog.method_defined?(:frozen?)    # inherited from Object/Kernel
