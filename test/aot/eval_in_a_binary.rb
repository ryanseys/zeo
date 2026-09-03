# `eval` keeps the compiler itself in the linked binary: the program compiles
# new code after it starts.
puts eval("1 + 2 * 3")
klass = Class.new do
  class_eval <<~RUBY
    def greet(name)
      "hello, \#{name}"
    end
  RUBY
end
puts klass.new.greet("world")
obj = Object.new
obj.instance_eval { @hidden = 41 }
puts obj.instance_variable_get(:@hidden) + 1
puts binding.local_variable_defined?(:obj)
__END__
7
hello, world
42
true
