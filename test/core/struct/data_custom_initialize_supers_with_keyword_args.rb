# an explicit keyword `super(x: .., y: ..)` binds the parent's
# keyword params by name; omitting a required one raises CRuby's
# `missing keyword` ArgumentError.

Point = Data.define(:x, :y) do
  def initialize(x:, y:)
    super(x: x * 100, y: y + 1)
  end
end
def mk(a, b); Point.new(x: a, y: b); end
p mk(5, 2)
Pair = Data.define(:m, :n) do
  def initialize(m:, n:)
    super(m: m)
  end
end
begin
  Pair.new(m: 1, n: 2)
rescue ArgumentError => e
  puts "err: #{e.message}"
end
__END__
#<data Point x=500, y=3>
err: missing keyword: :n
