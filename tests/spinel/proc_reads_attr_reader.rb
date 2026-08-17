class Scale
  attr_reader :factor
  def initialize(f); @factor = f; end
  def make; ->(n) { n * factor }; end
end
p Scale.new(3).make.call(2)
class S2
  attr_accessor :k
  def initialize(k); @k = k; end
  def pr; proc { |n| n + k }; end
  def lm; lambda { |n| n * k }; end
end
p S2.new(4).pr.call(1)
p S2.new(4).lm.call(2)
