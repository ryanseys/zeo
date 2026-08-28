# `is_a?` on a receiver whose static type is a BASE class. That type is only an
# upper bound -- a Base-typed slot legitimately holds a Sub, which is the whole
# point of a subclass -- so the test has to read the object's runtime class.
# It read the static one, so a predicate written on the base class answered
# false for every subclass instance, silently, while the identical test written
# at the call site answered true because there the receiver's type IS Sub.
#
# The downstream shape is Rails single-table inheritance, where the predicates
# live on the base class by construction (#4142).
class Base
  def inside = self.is_a?(Sub)
  def implicit = is_a?(Sub)
  def exact = instance_of?(Sub)
  def own = is_a?(Base)
end

class Sub < Base
end

class Deeper < Sub
end

b = Sub.new
p b.inside
p b.implicit
p b.exact
p b.own
p b.is_a?(Sub)
p b.is_a?(Base)

p Base.new.inside
p Base.new.own
p Base.new.exact

# Through the grandparent, and exact-vs-ancestor at each level.
d = Deeper.new
p [d.inside, d.exact, d.own]
p [d.is_a?(Deeper), d.is_a?(Sub), d.is_a?(Base), d.instance_of?(Sub)]

# A collection typed by the base, dispatching per element -- the STI shape.
rooms = [Base.new, Sub.new, Deeper.new]
p rooms.map { |r| r.inside }
p rooms.map { |r| r.class.to_s }

# A leaf class with no subclasses keeps the exact answer.
class Alone
  def me = is_a?(Alone)
end
p Alone.new.me

# The shape this arrived as: Rails single-table inheritance, where the
# predicates live on the base class by construction and the subclasses are
# declared after it. Every one of them answered false for every room -- a
# well-formed page with the wrong content, and no error anywhere.
class Room
  def open?   = is_a?(Rooms::Open)
  def closed? = is_a?(Rooms::Closed)
  def label = open? ? "open" : (closed? ? "closed" : "plain")
end

module Rooms
  class Open < Room; end
  class Closed < Room; end
end

rooms = [Rooms::Open.new, Rooms::Closed.new, Room.new]
p rooms.map { |r| r.label }
p rooms.map { |r| [r.open?, r.closed?] }
