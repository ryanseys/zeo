class Pairs
  include Enumerable
  def each; yield :a, 1; yield :b, 2; end
end
p(Pairs.new.map { |x001| x001 })

p(Pairs.new.map { |k002, v002| [k002, v002] })   # => [[:a, 1], [:b, 2]]
p(Pairs.new.select { |x003| true })              # => [[:a, 1], [:b, 2]]
p(Pairs.new.find { |x004| true })                # => [:a, 1]
p(Pairs.new.sort_by { |x005| x005.to_s })        # => [[:a, 1], [:b, 2]]
p(Pairs.new.count)                               # => 2
Pairs.new.each { |x006| p x006 }                 # => :a then :b
def y007; yield :a, 1; end
p(y007 { |x007| x007 })                          # => :a
