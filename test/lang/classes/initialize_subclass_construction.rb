# The reroute trap: builtin initialize rows must NOT make subclass
# construction think a USER initialize exists. Every shape below goes
# through the value-subclass constructor.
class Stack < Array; end
p Stack.new(3, 0)
p Stack.new(2) { |i| i * 5 }
p Stack.new.class

class Registry < Hash; end
r = Registry.new(0)
p [r.class, r[:missing]]

class Tag < String; end
p Tag.new("x")

# A user initialize STILL wins, and super still re-seeds the payload.
class Sized < Array
  def initialize(n)
    super(n, :x)
  end
end
p Sized.new(2)

# And the private rows resolve on the subclass through the ancestry.
s = Stack.new(1, 1)
s.send(:initialize, 2, 9)
p s
__END__
[0, 0, 0]
[0, 5]
Stack
[Registry, 0]
"x"
[:x, :x]
[9, 9]
