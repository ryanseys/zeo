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
