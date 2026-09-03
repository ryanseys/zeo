# Data.define synthesizes an immutable value class with keyword
# construction, deconstruct_keys (hash patterns), to_h, with, ==, inspect.

Coord = Data.define(:x, :y)
c = Coord.new(x: 1, y: 2)
p c
p c.x
p c.to_h
p c.with(y: 9)
p c
puts(c == Coord.new(x: 1, y: 2))
def take(d); case d; in {x:, y:}; x + y; end; end
p take(c)
begin
  Coord.new(x: 1)
rescue ArgumentError => e
  puts "err: #{e.message}"
end
__END__
#<data Coord x=1, y=2>
1
{x: 1, y: 2}
#<data Coord x=1, y=9>
#<data Coord x=1, y=2>
true
3
err: missing keyword: :y
