class Tags
  include Enumerable
  def initialize(*xs); @xs = xs; end
  def each; @xs.each { |x| yield x }; end
end

freq = Tags.new(1, 2, 3).reduce({}) { |h, x| h[x] = x * x; h }
p freq
