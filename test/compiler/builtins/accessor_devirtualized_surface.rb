# A method whose whole body is one ivar access reaches the field directly
# from the dynamic dispatch table (`zeo_tramp!`'s `rd`/`wr` heads) instead of
# downcasting and calling the generated method. Everything OBSERVABLE about
# such a method must survive that: reflection, visibility, arity errors, the
# frozen guard, inheritance and aliasing.
#
# A RUNTIME redefinition winning over an accessor is the one part not pinned
# here -- zeo does not honour it for a plain method either, so it lives in
# `test/lang/methods/issue_runtime_redefine_accessor.rb` rather than pretending to be
# something devirtualization introduced.
class Node
  attr_accessor :value
  attr_reader :tag

  # Hand-written, not `attr_*` -- the SHAPE is what qualifies, not the
  # provenance, so these must devirtualize identically.
  def left
    @left
  end

  def left=(n)
    @left = n
  end

  # One statement too many, or the wrong statement: NOT an accessor.
  def value_twice
    @value + @value
  end

  def touched
    @count = 1
    @count
  end

  def initialize(v)
    @value = v
    @tag = "t#{v}"
  end

  private

  def secret
    @tag
  end
end

n = Node.new(7)
p [n.value, n.tag, n.left]
n.value = 9
n.left = Node.new(1)
p [n.value, n.left.value, n.value_twice, n.touched]

# Reading an ivar nothing ever assigned is nil, and leaves it UNDEFINED.
p n.instance_variables.sort
fresh = Node.allocate
p [fresh.value, fresh.instance_variables]

# Reflection reaches the same method however it is spelled.
p [n.method(:value).arity, n.method(:value=).arity, n.method(:left).arity]
p [Node.instance_method(:value).owner, Node.instance_method(:left).owner]
p [n.method(:tag).parameters, n.method(:value=).parameters.map(&:first)]
p [n.respond_to?(:value), n.respond_to?(:secret), n.respond_to?(:secret, true)]
p [n.send(:value), n.send(:secret), n.public_send(:tag)]
p n.method(:value).unbind.bind(Node.new(3)).call

# A private accessor stays private through the dynamic path.
begin
  n.secret
rescue NoMethodError => e
  puts e.message
end

# Arity errors are attributed to the accessor, with CRuby's wording.
begin
  n.send(:value, 1)
rescue ArgumentError => e
  puts e.message
end
begin
  n.send(:value=)
rescue ArgumentError => e
  puts e.message
end

# The frozen guard survives, and reports the receiver.
frozen = Node.new(2)
frozen.freeze
begin
  frozen.value = 5
rescue FrozenError => e
  puts e.message.sub(/0x[0-9a-f]+/, "0xADDR")
  p e.receiver.equal?(frozen)
end
p frozen.value

# Inherited and aliased accessors keep working, and keep their identities.
class Child < Node
  alias_method :val, :value
end

c = Child.new(4)
p [c.value, c.val, c.tag]
p [Child.instance_method(:value).owner, c.method(:val).original_name]
c.val = 6 if c.respond_to?(:val=)
p c.value
__END__
[7, "t7", nil]
[9, 1, 18, 1]
[:@count, :@left, :@tag, :@value]
[nil, []]
[0, 1, 0]
[Node, Node]
[[], [:req]]
[true, false, true]
[9, "t7", "t7"]
3
private method 'secret' called for an instance of Node
wrong number of arguments (given 1, expected 0)
wrong number of arguments (given 0, expected 1)
can't modify frozen Node: #<Node:0xADDR @value=2, @tag="t2">
true
2
[4, 4, "t4"]
[Node, :value]
4
