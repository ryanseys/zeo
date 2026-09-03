# #192 feature (1): a `super` inside a method defined in a NON-const
# `Struct.new`/`Data.define` block resolves through the runtime method-frame
# stack into the native member-binding `initialize`, where previously it
# panicked "`super` outside a method" at codegen. Bare (zsuper) and explicit
# positional forms, plus the keyword form for Data.

pair = Struct.new(:x, :y) do
  def initialize(x)
    super(x, x * 2)
  end
end
p pair.new(5).to_a

echo = Struct.new(:a, :b) do
  def initialize(a, b)
    super
  end
end
p echo.new(1, 2).to_a

coord = Data.define(:lat, :lng) do
  def initialize(lat:, lng:)
    super(lat: lat * 10, lng: lng)
  end
end
p coord.new(lat: 1, lng: 2)
__END__
[5, 10]
[1, 2]
#<data lat=10, lng=2>
