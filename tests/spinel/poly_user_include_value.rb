# `include?` on a poly receiver whose class defines its own: the dispatch
# accumulates the user arm and the builtin ones in one C temp, and the temp was
# typed from the builtin -- a bool. The method's value came back through it as
# true/false, so a position answer of `0`, which Ruby reads as truthy, arrived
# as false (#4072). The call has to be typed for what both arms can hold.
class Tags
  def initialize(names)
    @names = names
    @asked = 0
  end

  attr_reader :asked

  # answers where it found the name; Ruby reads 0 as truthy
  def include?(name)
    @asked += 1
    @names.index(name)
  end
end

def found(value) = value.include?("ruby")

tags = Tags.new(["ruby", "spinel"])
p found(tags)
p tags.asked
p found(["ruby"])
p found(["spinel", "ruby"])
p found(["none"])
p found("a ruby string")
p found({ "ruby" => 1 })

# a class whose include? really does answer a bool keeps the plain path
class Bag
  def initialize(xs) = @xs = xs
  def include?(x) = @xs.length > 0 && x == "y"
end

def has(v) = v.include?("y")
p has(Bag.new(["y"]))
p has(["y"])
p has(["n"])
