# The eval VM's `defined?` and `(...)` forwarding -- forwardable's generated
# delegators need both, so every gem that uses `def_delegators` does.
class Box
  def initialize(inner) = @inner = inner
end

Box.class_eval(<<~RUBY)
  def call_it(...)
    _ = @inner
    if defined?(_.upcase)
      _.upcase(...)
    else
      :no_upcase
    end
  end

  def probe
    x = 1
    [defined?(x), defined?(y), defined?(@inner), defined?(@nope),
     defined?(String), defined?(NotAThing), defined?(self), defined?(1 + 1),
     defined?(probe), defined?(no_such_method)]
  end
RUBY

p Box.new("hi").call_it
p Box.new(42).call_it
p Box.new("x").probe

# Forwarding carries the BLOCK too, not just the positional arguments.
class Relay
  def initialize(target) = @target = target
end
Relay.class_eval(<<~RUBY)
  def each(...)
    @target.each(...)
  end
RUBY
seen = []
Relay.new([1, 2, 3]).each { |v| seen << v * 2 }
p seen

# A `do ... end` block inside eval re-parses correctly (it is stored as its own
# source and wrapped back into call position on each invocation).
g = eval("proc do\n  def made_here(a, b = 2)\n    a + b\n  end\nend")
class Target; end
Target.module_eval(&g)
p Target.new.made_here(1)
p Target.new.made_here(1, 10)
