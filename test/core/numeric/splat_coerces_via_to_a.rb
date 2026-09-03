# A call-site/yield splat coerces its operand the way ruby does -- `to_a`
# when it responds (rspec splats its own Set into `register_listener`),
# wrap otherwise, nil to nothing -- never a bare Array unwrap.
class MySet
  include Enumerable
  def initialize(items) = @items = items
  def each(&b) = @items.each(&b)
  def to_a = @items.dup
end
def takes(*xs) = xs.inspect
puts takes(*MySet.new([1, 2]))
puts takes(*3)
puts takes(*nil)
def yields
  yield(*MySet.new([4, 5]))
end
yields { |a, b| puts "#{a}/#{b}" }
__END__
[1, 2]
[3]
[]
4/5
