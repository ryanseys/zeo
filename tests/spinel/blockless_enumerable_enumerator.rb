class Nums
  include Enumerable
  def initialize(*xs); @xs = xs; end
  def each; @xs.each { |x| yield x }; end
end
r001 = (Nums.new(1, 2, 3).sort_by.class rescue $!.class); p r001
r002 = (Nums.new(1, 2, 3).group_by.class rescue $!.class); p r002
r003 = (Nums.new(1, 2, 3).min_by.class rescue $!.class); p r003
r004 = (Nums.new(1, 2, 3).find.class rescue $!.class); p r004
p Nums.new(1,2,3).map.class
p Nums.new(1,2,3).select.class
p [1,2,3].sort_by.class
p [1,2,3].group_by.class
p [1,2,3].min_by.class
p [1,2,3].find.class
p [3,1,2].sort_by { |x| x }
p [1,2,3].find { |x| x > 1 }
p [1,2,3].group_by { |x| x.odd? }
