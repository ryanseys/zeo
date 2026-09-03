# GAP -- imported from the spinel corpus at c55d9bdb.
# A user object nested in a JSON structure is rendered by inspect rather
# than its own to_json.
#
require "json"

class Point
  def initialize(x, y)
    @x = x
    @y = y
  end

  def to_json(*args)
    { x: @x, y: @y }.to_json(*args)
  end
end

p Point.new(1, 2).to_json
p JSON.generate({ points: [Point.new(1, 2), Point.new(3, 4)], n: 2 })
puts JSON.pretty_generate([Point.new(5, 6)])
__END__
"{\"x\":1,\"y\":2}"
"{\"points\":[{\"x\":1,\"y\":2},{\"x\":3,\"y\":4}],\"n\":2}"
[
  {
    "x": 5,
    "y": 6
  }
]
