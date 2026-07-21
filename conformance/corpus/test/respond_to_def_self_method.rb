# def self.m singleton methods must be visible to respond_to? on the
# class constant, and module_function methods on the module constant.
class Foo
  def self.bar; 1; end
end
puts Foo.respond_to?(:bar)
puts Foo.respond_to?(:nope)

module M
  module_function
  def greet; "hi"; end
end
puts M.respond_to?(:greet)
puts M.respond_to?(:nope)
