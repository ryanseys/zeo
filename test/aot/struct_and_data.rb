# Struct and Data build classes at run time; the binary carries the class
# machinery that needs.
Pair = Struct.new(:left, :right) do
  def sum = left + right
end
Coord = Data.define(:lat, :lon)
pair = Pair.new(2, 3)
puts pair.sum, pair.to_a.inspect
c = Coord.new(lat: 45.4, lon: -75.7)
puts c.lat, c.with(lon: 0).inspect
p Pair.members, Coord.members
__END__
5
[2, 3]
45.4
#<data Coord lat=45.4, lon=0>
[:left, :right]
[:lat, :lon]
