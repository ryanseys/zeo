# respond_to? on a class/module value must see user `def self.x`,
# module_function, and class<<self accessors, plus inherited builtin
# Class methods -- but a module never responds to :new.

class Foo
  def self.custom; end
end
module Bar
  def self.helper; end
  module_function
  def mf; end
end
module Acc
  class << self
    attr_accessor :x
  end
end
puts Foo.respond_to?(:new)
puts Foo.respond_to?(:custom)
puts Foo.respond_to?(:name)
puts Foo.respond_to?(:nope_xyz)
puts Bar.respond_to?(:new)
puts Bar.respond_to?(:helper)
puts Bar.respond_to?(:mf)
puts Acc.respond_to?(:x)
puts Acc.respond_to?(:x=)
__END__
true
true
true
false
false
true
true
true
true
