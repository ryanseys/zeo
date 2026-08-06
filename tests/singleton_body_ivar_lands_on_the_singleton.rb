# An `@x = v` inside `class << self` writes an ivar of the SINGLETON class,
# which is a different object from the class -- so an `attr_accessor` written
# beside it (a CLASS method, whose self is the class) does not read it back.

class Widget
  class << self
    @registry = "singleton-ivar"

    attr_accessor :registry

    def peek
      @registry
    end
  end
end

p Widget.registry
p Widget.peek
p Widget.singleton_class.instance_variable_get(:@registry)
p Widget.instance_variable_get(:@registry)

Widget.registry = "set-on-the-class"
p Widget.registry
p Widget.singleton_class.instance_variable_get(:@registry)
