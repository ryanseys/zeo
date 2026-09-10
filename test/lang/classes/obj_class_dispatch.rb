# `obj.class` on an instance answers the real class: a class method called
# through it, `.to_s`, the chain `obj.class.method`, and the
# `"#{self.class}"` interpolation an inherited `inspect` uses.

class Foo
  def self.greet
    "hello-from-foo"
  end
end

f = Foo.new
puts f.class.greet              # hello-from-foo
puts f.class.to_s               # Foo

class Bar
  attr_accessor :v
  def initialize
    @v = 0
  end
  def describe
    "#<#{ self.class }>"
  end
end

b = Bar.new
puts b.describe                 # #<Bar>

# Subclass: dispatch should pick up the cmeth defined on the
# inherited class: the lookup walks the parents.
class Base
  def self.kind
    "base-kind"
  end
end

class Child < Base
end

c = Child.new
puts c.class.kind               # base-kind
__END__
hello-from-foo
Foo
#<Bar>
base-kind
