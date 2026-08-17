class Pair
  def to_ary = [1, 2]

  def inspect = "#<Pair>"
end

p [Pair.new].flatten
p [Pair.new].flatten(1)
p [[1, Pair.new]].flatten
p [Pair.new].flatten!
