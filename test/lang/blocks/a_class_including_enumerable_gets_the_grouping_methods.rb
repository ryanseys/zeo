# A class defining only each answers the Enumerable methods over its elements.
class Tags
  include Enumerable
  def initialize(*xs); @xs = xs; end
  def each; @xs.each { |x| yield x }; end
end

freq = Tags.new(1, 2, 3).reduce({}) { |h, x| h[x] = x * x; h }
p freq
__END__
{1 => 1, 2 => 4, 3 => 9}
