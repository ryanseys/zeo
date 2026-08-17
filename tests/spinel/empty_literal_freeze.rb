# `[].freeze` is the frozen-empty-array idiom: the call must not be dropped,
# and the array must be built at the slot's type (#3828).
p [].freeze.frozen?
p [1].freeze.frozen?
p({}.freeze.frozen?)
p "s".freeze.frozen?
x = [].freeze
p x.frozen?
p x
y = []
y.freeze
p y.frozen?
EMPTY = [].freeze
p EMPTY.frozen?
begin
  x << 1
rescue FrozenError
  puts "FrozenError"
end
