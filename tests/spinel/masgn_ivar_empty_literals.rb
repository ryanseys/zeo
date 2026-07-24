# Instance-variable multi-assignment with empty hash/array literals: the
# element temps adopt the ivar's type (poly ivars take a fresh boxed
# container), and the assignment boxes from the temp's resolved type (#3280).
class MyClass
  attr_reader :foo, :bar, :baz, :qux, :quux
  def initialize
    @foo, @bar = {}, {}
    @baz, @qux, @quux = [], [], []
  end
end
m = MyClass.new
p m.foo
p m.bar
p m.baz
p m.quux
puts "OK"
