# `class Vec` would otherwise define a crate-root `Vec` struct shadowing
# Rust's own, breaking every `Vec<RubyValue>` in the generated program.

class Vec
  def initialize(x, y)
    @x = x
    @y = y
  end
  attr_reader :x, :y
  def +(other) = Vec.new(@x + other.x, @y + other.y)
  def to_s = "Vec(#{@x}, #{@y})"
end
class Option
  def initialize(v) = @v = v
  def some? = !@v.nil?
end
class Box
  def initialize(v) = @v = v
  def get = @v
end
puts(Vec.new(1, 2) + Vec.new(10, 20))
p Vec.new(1, 2).class.name
p Option.new(nil).some?
p Box.new(:b).get
__END__
Vec(11, 22)
"Vec"
false
:b
