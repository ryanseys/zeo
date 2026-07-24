Item = Struct.new(:cat, :qty) do
  def value = qty
end
items = [Item.new(:a, 1), Item.new(:b, 2), Item.new(:a, 3)]
items.group_by(&:cat).each { |cat, g| p [cat, g.sum(&:value)] }

Rec = Struct.new(:name, :amt) do
  def cost = amt
end
recs = [Rec.new("x", 1.5), Rec.new("y", 2.5), Rec.new("x", 3.0)]
by = recs.group_by(&:name)
by.each { |k, g| p [k, g.sum(0.0, &:cost)] }
by.each { |k, g| p [k, g.sum { |r| r.cost * 2 }] }

strs = { p: %w[a b], q: %w[c] }
strs.each { |k, g| p g.sum("") { |s| s } }
