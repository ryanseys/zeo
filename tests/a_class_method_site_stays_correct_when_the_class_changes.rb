# A `Target.name` call site caches the class method it resolved. Everything
# that can make that answer wrong must still win.

module Shape
  def self.area(n) = n * n
  def self.secret = :hidden
  private_class_method :secret
end

3.times { print Shape.area(3), " " }
puts

begin
  Shape.secret
rescue NoMethodError => e
  p e.class
end

# A runtime singleton definition beats a site that already filled.
class Counter
  def self.tick = 1
end
p [Counter.tick, Counter.tick]
def Counter.tick = 2
p [Counter.tick, Counter.tick]

# An `extend`ed module supplies a class method through the singleton ancestry.
module Greet
  def hello = "hi"
end
class Host
  extend Greet
end
p [Host.hello, Host.hello]

# `method_missing` on the singleton answers where no method exists.
class Ghost
  def self.method_missing(n, *a) = [n, a]
  def self.respond_to_missing?(_n, _p = false) = true
end
p Ghost.anything(1, 2)
p Ghost.anything(1, 2)

# Builtin class methods keep working under repetition.
p [Integer.sqrt(16), Integer.sqrt(16)]
p [Math.hypot(3, 4), Math.hypot(3, 4)]
p [String.new("x"), String.new("x")]

# A minted Struct class's own singleton methods shadow `Struct.new`.
Point = Struct.new(:x, :y)
p Point.members
p Point[1, 2].to_a
p Point.new(3, 4).to_a

# A class method reached through inheritance.
class Base
  def self.tag = "base"
end
class Sub < Base; end
p [Sub.tag, Sub.tag]

# The same name on two classes must not share one site's answer.
class A1
  def self.who = :a
end
class B1
  def self.who = :b
end
2.times { print A1.who, B1.who, " " }
puts
