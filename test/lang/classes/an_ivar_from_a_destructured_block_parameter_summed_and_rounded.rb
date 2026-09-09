# Objects built inside a map over pairs, their areas summed with a block and
# rounded.
# (spinel issue #2933)
class Shape
  def initialize(s) = @s = s
  def area = @s * 3.14
end

shapes = [["a", 2], ["b", 3]].map { |_, s| Shape.new(s) }
p shapes.sum { |x| x.area }.round(2)
__END__
15.7
