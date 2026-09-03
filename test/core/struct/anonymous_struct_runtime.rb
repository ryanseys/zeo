# A `Struct.new` / `Data.define` OUTSIDE a constant assignment mints a native
# class at runtime (Batch E), so a struct can be an anonymous local, used
# inline, or built in a loop -- not only `Name = Struct.new(...)`. The constant
# form is still compile-time synthesized (so `super`/subclassing keep working).

# anonymous struct assigned to a local
pair = Struct.new(:a, :b)
o = pair.new(1, 2)
p o
p o.a
p o.to_a
p o.members
o.a = 10
p o.a

# Enumerable, and a class-body method block
point = Struct.new(:x, :y) do
  def dist
    Math.sqrt(x**2 + y**2)
  end
end
pt = point.new(3, 4)
p pt.dist
p pt.map { |v| v * 2 }
p pt.select(&:even?)

# used inline
p Struct.new(:n).new(42).n

# an anonymous Data class: keyword construction, immutable, `with`
coord = Data.define(:lat, :lng)
c = coord.new(lat: 51, lng: 0)
p c
p c.lat
p c.to_h
p c.with(lng: 13)
p c.frozen?

# a struct built per row in a loop
rows = [[1, 2], [3, 4]].map do |a, b|
  Struct.new(:a, :b).new(a, b)
end
rows.each { |r| p r.to_a }
__END__
#<struct a=1, b=2>
1
[1, 2]
[:a, :b]
10
5.0
[6, 8]
[4]
42
#<data lat=51, lng=0>
51
{lat: 51, lng: 0}
#<data lat=51, lng=13>
true
[1, 2]
[3, 4]
