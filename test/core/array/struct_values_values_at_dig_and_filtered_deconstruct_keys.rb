Point = Struct.new(:x, :y)
p Point.new(3, 4).values
p Point.new(3, 4).values_at(0, -1)
Config = Struct.new(:name, :opts)
c = Config.new("web", { port: 8080 })
p c.dig(:opts, :port)
p c.dig(:opts, :missing)
S = Struct.new(:a, :b, :c)
p S.new(1, 2, 3).deconstruct_keys([:a, :c])
p S.new(1, 2, 3).deconstruct_keys([:z, :a])
p S.new(1, 2, 3).deconstruct_keys([:a, :b, :c, :d])
D = Data.define(:a, :b)
p D.new(a: 1, b: 2).deconstruct_keys([:a])
p D.new(a: 1, b: 2).deconstruct_keys(nil)
__END__
[3, 4]
[3, 4]
8080
nil
{a: 1, c: 3}
{}
{}
{a: 1}
{a: 1, b: 2}
