# `Data.define` builds an IMMUTABLE value class: keyword construction, readers
# (no writers), structural `==`, and `deconstruct`/`deconstruct_keys` for
# pattern matching. `with` returns a copy with some members replaced.
Point = Data.define(:x, :y)

p1 = Point.new(x: 1, y: 2)
p p1                            # #<data Point x=1, y=2>
p p1.x                         # 1
p p1.to_h                      # {x: 1, y: 2}
p p1.members                   # [:x, :y]

# Immutable: `with` returns a NEW value, leaving the original untouched.
p2 = p1.with(y: 9)
p p2                           # #<data Point x=1, y=9>
p p1                           # #<data Point x=1, y=2>  (unchanged)

# Structural equality by class + members.
p(p1 == Point.new(x: 1, y: 2)) # true
p(p1 == p2)                    # false

# Pattern matching deconstructs by keys...
def describe(pt)
  case pt
  in { x: 0, y: } then "on the y-axis at #{y}"
  in { x:, y: }   then "at (#{x}, #{y})"
  end
end
puts describe(Point.new(x: 0, y: 5))
puts describe(Point.new(x: 3, y: 4))

# ...or positionally.
case p1
in [a, b] then p [a, b]        # [1, 2]
end

# A missing required member on construction is an ArgumentError.
begin
  Point.new(x: 1)
rescue ArgumentError => e
  puts "error: #{e.message}"   # missing keyword: :y
end
