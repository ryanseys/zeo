# `inherit: false` reports only methods the receiver defines directly --
# its own defs and attr accessors -- skipping the ancestor walk.

class Animal; def name; end; attr_accessor :age; end
class Dog < Animal; def bark; end; end
p Dog.method_defined?(:bark, false)
p Dog.method_defined?(:name, false)
p Dog.method_defined?(:age=, false)
p Animal.method_defined?(:age, false)
p Animal.method_defined?(:bark, false)
__END__
true
false
false
true
false
