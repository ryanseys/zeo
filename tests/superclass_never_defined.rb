# A superclass naming a constant the program never defines. Zeo used to
# reject the whole compile ("unknown superclass"), which meant a library that
# subclasses something from a gem it failed to load -- irb's `class CallTracer
# < ::CallTracer`, guarded by a `require "tracer"` that already raised
# LoadError -- could not be built at all, even though the definition never
# runs. Real Ruby evaluates the superclass expression when the definition
# RUNS, raises NameError there, and leaves no class behind.
class Base
  def who = :base
end

class Ordinary < Base
end

puts Ordinary.new.who
puts Ordinary.superclass

begin
  class Sub < ::Nope
    def hi = :hi
  end
rescue NameError => e
  puts e.class
  puts e.message
end

p defined?(Sub)
p Object.const_defined?(:Sub)

# The name elsewhere is an ordinary unresolved constant read, so it raises the
# same way rather than silently answering an empty class.
begin
  Sub.new
rescue NameError => e
  puts e.message
end
