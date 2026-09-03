# Module#constants (own first, ancestors' next, Object's excluded),
# #included_modules (modules in the MRO), and #class_variables (own +
# ancestors).

module Walks; end
class Animal; end
class Dog < Animal; include Walks; end
p Dog.included_modules
module Mod; MC = 9; end
class A; X = 1; end
class B < A; Y = 2; include Mod; end
p A.constants
p B.constants.sort
p B.constants(false)
class C; @@x = 5; end
C.class_variable_set(:@@y, 9)
p C.class_variables.sort
__END__
[Walks, Kernel]
[:X]
[:MC, :X, :Y]
[:Y]
[:@@x, :@@y]
