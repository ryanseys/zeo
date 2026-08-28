# A bare call inside a method resolves the way CRuby's ancestry does: the
# enclosing class's own chain answers it before a top-level `def` does, because
# a top-level def lands on Object, which is BELOW the class. Spinel kept
# top-level defs in a flat table consulted ahead of the class members, so a
# helper in a class lost to a same-named helper beside it.
def helper = "top"
def size = 9
def name = "top-name"

class K
  def helper = "class"
  def size = 5
  def run = helper
  def sz = size
end

class Base
  def helper = "base"
end

class Sub < Base
  def run = helper
end

class NoneOfIt
  def run = helper
end

class Attr
  attr_reader :name
  def initialize
    @name = "attr"
  end

  def run = name
end

p K.new.run
p K.new.sz
p Sub.new.run
p NoneOfIt.new.run
p Attr.new.run
p helper
p size
p name

# a class method's bare call sees its own class first as well
class Counter
  def self.bump = tick
  def self.tick = 3
end
def tick = 99
p Counter.bump
p tick
