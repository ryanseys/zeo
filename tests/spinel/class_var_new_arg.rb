class Point
  def initialize(x); @x = x; end
  def to_s; "P(#{@x})"; end
end

pt = Point
p pt.new(7).to_s
