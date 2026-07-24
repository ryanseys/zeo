# Ruby names that collide with Rust's own vocabulary must not leak into the
# generated program: a class named after a Rust prelude type, a method named
# after a Rust keyword, and Ruby's `_` local (which is NOT a named binding in
# Rust). All are mangled by `codegen::ident`; none is observable from Ruby.

# --- Classes named after Rust prelude types -------------------------------

class Vec
  def initialize(x, y)
    @x = x
    @y = y
  end
  attr_reader :x, :y

  def +(other)
    Vec.new(@x + other.x, @y + other.y)
  end

  def to_s = "Vec(#{@x}, #{@y})"
end

v = Vec.new(1, 2) + Vec.new(10, 20)
puts v
p v.x
p v.class
p v.class.name
p v.is_a?(Vec)

class Option
  def initialize(value) = @value = value
  def some? = !@value.nil?
  def value_or(default) = @value.nil? ? default : @value
end
p Option.new(5).some?
p Option.new(nil).some?
p Option.new(nil).value_or(:fallback)
p Option.new(:got).value_or(:fallback)

class Box
  def initialize(v) = @v = v
  def get = @v
end
p Box.new("boxed").get

class Result
  def initialize(ok) = @ok = ok
  def ok? = @ok
end
p Result.new(true).ok?

# Inheritance between prelude-named classes.
class Iterator
  def each = [1, 2].each { |i| yield i }
end
class Send < Iterator; end
out = []
Send.new.each { |i| out << i }
p out
p Send.new.is_a?(Iterator)

# A prelude-named class used as a Hash key and in a collection.
registry = { Vec => :vector, Option => :option }
p registry[Vec]
p [Vec.new(0, 0), Vec.new(1, 1)].map(&:to_s)

# --- Methods named after Rust keywords ------------------------------------

class Keywords
  def type = "a type"
  def match = "a match"
  def loop = "a loop"
  def fn = "an fn"
  def impl = "an impl"
  def move = "a move"
  def ref = "a ref"
  def struct = "a struct"
end
k = Keywords.new
p k.type
p k.match
p k.loop
p k.fn
p k.impl
p k.send(:type)
p k.respond_to?(:match)

# Predicate/bang/setter suffixes.
class Suffixes
  def initialize = @saved = false
  def empty? = false
  def save! = @saved = true
  def value=(v)
    @value = v
  end
  def value = @value
  def saved? = @saved
end
s = Suffixes.new
p s.empty?
s.save!
p s.saved?
s.value = 42
p s.value

# --- Ruby's `_` local ------------------------------------------------------

# `_` is an ordinary, readable local in Ruby.
_ = 10
p _
_ = _ + 5
p _

# The conventional throwaway in a block, still readable.
[[1, :a], [2, :b]].each do |n, _|
  p n
end
[[1, :a]].each { |_, sym| p sym }

# `_` in a multiple assignment.
_, second = [:first, :second]
p second
p _

# Repeated `_` params (Ruby allows the duplicate name).
[[1, 2, 3]].each { |_, _, third| p third }
