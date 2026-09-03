Point = Data.define(:x, :y)
p1 = Point.new(1, 2)
p(p1.deconstruct_keys([:x, :nope]))
p(p1.deconstruct_keys([:x, :y]))
p(p1.deconstruct_keys(nil))
p(p1.deconstruct_keys([:nope]))
__END__
{x: 1}
{x: 1, y: 2}
{x: 1, y: 2}
{}
