# `define_method(name, &value)` -- the body is a value the program computes, so
# there is no block to desugar into a `def`. Every gem that builds methods in a
# loop uses it; zeo rejected it at compile time even though the runtime
# `Module#define_method` installs a Proc, a Method or an UnboundMethod body.

class Greeter
  say = proc { |n| "hi #{n}" }
  define_method(:greet, &say)
  define_method(:now, &::Time.method(:now))

  %w[one two].each_with_index do |name, i|
    define_method(name) { i }
  end
end

g = Greeter.new
p g.greet("world")
p g.now.class
p [g.one, g.two]
p Greeter.instance_method(:greet).arity
__END__
"hi world"
Time
[0, 1]
1
