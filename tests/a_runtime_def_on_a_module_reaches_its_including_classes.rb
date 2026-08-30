module Greet
  def hello = :first
end

class Host
  include Greet
end

h = Host.new
p h.hello

# `module_eval` reaching a MODULE at run time. The including class carries a
# flattened copy of the module's row, and it has to stop answering here.
def redefine_by_module_eval
  Greet.module_eval { def hello = :by_module_eval }
end
redefine_by_module_eval
p h.hello

# `class_eval` on a module is the same door.
def redefine_by_class_eval
  Greet.class_eval { def hello = :by_class_eval }
end
redefine_by_class_eval
p h.hello

# ...and so is `define_method`.
def redefine_by_define_method
  Greet.send(:define_method, :hello) { :by_define_method }
end
redefine_by_define_method
p h.hello

# A module mixed into a CORE class, which is the shape the bug was found on.
module Kernel
  def late = :a
end
def bump
  Kernel.module_eval { def late = :b }
end
p 5.late
bump
p 5.late

# The module itself answers the same body, and a fresh instance agrees with
# the one made before any of the redefinitions.
p Host.new.hello
p Greet.instance_method(:hello).owner
