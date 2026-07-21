# A class/module object responds to the builtin Class/Module methods it
# inherits (:name, :new, :instance_methods, :superclass, ...), not only its
# user-defined singleton methods.
class Foo
  def self.custom; end
end
module Bar
  def self.helper; end
end
puts Foo.respond_to?(:new)               # true
puts Foo.respond_to?(:name)              # true
puts Foo.respond_to?(:instance_methods)  # true
puts Foo.respond_to?(:superclass)        # true
puts Foo.respond_to?(:custom)            # true (user singleton)
puts Foo.respond_to?(:nope_xyz)          # false
puts Bar.respond_to?(:new)               # false (module)
puts Bar.respond_to?(:name)              # true
puts Bar.respond_to?(:helper)            # true
puts "done"
