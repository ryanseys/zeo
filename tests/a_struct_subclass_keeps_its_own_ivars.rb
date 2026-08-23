# A subclass of a Struct inherits the member list, so an instance of it has to
# hold BOTH its ancestor's members and its own instance variables -- and the
# ancestor's compiled body addresses a member by slot index, so the two must
# not land on the same slot.
#
# They did. Members were laid out after the declared ivars, and a subclass
# declaring one of its own pushed them along: `@p` took slot 0, which is where
# the struct's `initialize` writes member 0. `super` then wrote the member's
# value into `@p`, and reading the member answered nil. rspec's
# `FailedExampleNotification` is the shape -- a `Struct.new(:example)`
# subclass whose `initialize` sets `@exception_presenter` and calls
# `super(example)` -- and it reported the example where the presenter belonged.
#
# Members now occupy the FIRST slots, which every descendant agrees on.

Base = Struct.new(:example)

class Sub < Base
  def initialize(example, presenter = "P<#{example}>")
    @presenter = presenter
    super(example)
  end
  def show = [@presenter, example, to_a]
end

s = Sub.new("ex")
p s.show
p s.instance_variables
p s
p Sub.new("ex", "given").show

# Two of its own, and a write through the member accessor afterwards.
class Two < Base
  def initialize(a, b)
    @a = a
    @b = b
    super("m")
  end
  def show = [@a, @b, example]
end
t = Two.new(1, 2)
p t.show
t.example = "changed"
p t.show
p t.instance_variables

# A deeper subclass adds a third ivar without moving the member again.
class Three < Two
  def initialize
    super(10, 20)
    @c = 30
  end
  def show3 = [@a, @b, @c, example]
end
p Three.new.show3
p Three.new.instance_variables

# The struct's own reflection still reports members, not ivars.
p Sub.members
p Sub.new("z").to_h
