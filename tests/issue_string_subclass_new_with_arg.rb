# Instantiating a String subclass with a constructor argument
# (`klass.new("x")`) raises ArgumentError ("wrong number of arguments") --
# zeo doesn't forward the argument to String's own initializer for a
# dynamically-created subclass.
klass = Class.new(String) do
  def foo
    42
  end
end
s = klass.new("x")
p s
p s.foo
