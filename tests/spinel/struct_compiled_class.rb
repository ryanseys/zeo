# A `NAME = Struct.new(:a, :b)` with a literal member list compiles to a real
# class whose members are slots rather than an overlay minted at runtime. What
# the program can observe must not change, so this pins the whole protocol
# against the oracle -- including the parts that are NOT ivars.

Point = Struct.new(:x, :y)
pt = Point.new(1, 2)

p pt.x, pt.y, pt.to_a, pt.members, Point.members, pt.size, pt.length
p pt == Point.new(1, 2), pt == Point.new(1, 3), pt.eql?(Point.new(1, 2))
p pt.hash == Point.new(1, 2).hash

# Members are storage, but they are not instance variables.
p pt.instance_variables
p pt.instance_variable_get(:@x)
p pt.instance_variable_defined?(:@x)

# An ivar invented on top of the members is a real, separate one.
pt.instance_variable_set(:@note, "hi")
p pt.instance_variables, pt.instance_variable_get(:@note), pt.x

p pt[0], pt[1], pt[-1], pt[:x], pt["y"]
pt[:x] = 10
pt.y = 20
p pt.to_a, pt.to_h, pt.inspect, pt.to_s

# Short and empty construction nil-fill rather than raise.
p Point.new(1).to_a, Point.new.to_a

begin
  Point.new(1, 2, 3)
rescue ArgumentError => e
  p e.class
end
begin
  pt[9]
rescue IndexError => e
  p e.class
end
begin
  pt[:nope]
rescue NameError => e
  p e.class
end

pt.each { |v| print v, " " }
puts
pt.each_pair { |k, v| print k, "=", v, " " }
puts

p pt.dig(0), pt.dig(:y), pt.values_at(0, 1), pt.values

case pt
in [a, b]
  p [a, b]
end
case pt
in {x:, y:}
  p [x, y]
end

p Marshal.load(Marshal.dump(Point.new(3, 4))).to_a
p pt.is_a?(Struct), Point.ancestors.include?(Struct), Point.superclass
p pt.dup.to_a, pt.clone.to_a
p Point.new(1, 2).frozen?

# Enumerable arrives through Struct, and so does `select`/`map`.
p pt.map { |v| v * 2 }
p pt.select { |v| v > 15 }
p pt.include?(10)

# A subclass of a compiled struct inherits the members.
class Point3 < Point
  def norm = x + y
end
q = Point3.new(4, 5)
p q.to_a, q.members, q.norm, q.x, q.is_a?(Point)

# A user method on the struct body itself, and a member reassigned inside it.
Boxed = Struct.new(:v)
class Boxed
  def bump
    self.v = v + 1
    self
  end
end
p Boxed.new(1).bump.v

# Frozen members raise on write, as any frozen object does.
f = Point.new(1, 2).freeze
begin
  f.x = 3
rescue FrozenError => e
  p e.class
end
