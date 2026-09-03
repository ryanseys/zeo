# Struct: values / values_at / dig (including nested dig into a Hash)
Point = Struct.new(:x, :y)
p Point.new(3, 4).values
p Point.new(3, 4).values_at(0, 1)
p Point.new(3, 4).values_at(-1)

Config = Struct.new(:name, :opts)
c = Config.new("web", { port: 8080, tags: %w[a b] })
p c.dig(:opts, :port)
p c.dig(:name)
p c.dig(:opts, :missing)

# The new rows also reach a custom-initialize Struct (two-level base split).
Measure = Struct.new(:amount, :unit) do
  def initialize(amount, unit = "kg")
    super(amount, unit)
  end
end
m = Measure.new(5)
p m.values
p m.values_at(1, 0)
p m.dig(:unit)

# Data still answers to_h / deconstruct_keys unchanged.
Coord = Data.define(:lat, :lng)
d = Coord.new(lat: 1, lng: 2)
p d.to_h
p d.deconstruct_keys([:lat])
p d.deconstruct
__END__
[3, 4]
[3, 4]
[4]
8080
"web"
nil
[5, "kg"]
["kg", 5]
"kg"
{lat: 1, lng: 2}
{lat: 1}
[1, 2]
