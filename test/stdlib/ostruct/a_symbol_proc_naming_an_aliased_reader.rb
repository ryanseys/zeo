# `select(&:required?)` where the predicate is an alias_method of the reader,
# and the same for a non-predicate alias.
# (spinel issue #3296)
require "ostruct"

class Item
  attr_reader :required
  def initialize = (@required = true)
  alias_method :required?, :required
end

OpenStruct.new
raise "FAIL" unless [Item.new].select(&:required?).length == 1
puts "ok"

class Point
  attr_reader :x
  def initialize(x) = (@x = x)
  alias_method :horizontal, :x
end

pts = [Point.new(3), Point.new(0)]
p pts.map(&:horizontal)
p pts.select(&:horizontal).length
__END__
ok
[3, 0]
2
