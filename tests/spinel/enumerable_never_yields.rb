class Empt
  include Enumerable
  def each; end
end
p(Empt.new.to_a)
p(Empt.new.map { |x| x })
p(Empt.new.count)
