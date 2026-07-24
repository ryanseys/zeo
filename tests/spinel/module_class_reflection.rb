# A module/class constant must behave as a first-class Module/Class
# object: .class, is_a?/kind_of?/instance_of?(Module|Class), and a
# nested name rendered with the `::` separator.
module Foo
  class Bar; end
end
module Outer
  module Inner; end
end
module M; end
class C; end

puts Foo::Bar.name           # nested name uses ::
puts Foo.class               # Module
puts C.class                 # Class
puts Outer::Inner.class      # Module (ConstantPathNode receiver)
puts M.is_a?(Module)         # true
puts C.is_a?(Module)         # true
puts M.is_a?(Class)          # false
puts C.is_a?(Class)          # true
puts M.is_a?(Object)         # true
puts M.instance_of?(Module)  # true
puts C.instance_of?(Class)   # true
puts M.instance_of?(Class)   # false
k = C                        # dynamic Class value
puts k.is_a?(Class)          # true
puts k.class                 # Class
