Point = Struct.new(:x, :y)
pts = [Point.new(1, 2), Point.new(3, 4)]
p pts.min_by { |pt| pt.x }.to_h
p pts.max_by { |pt| pt.x }.to_h
p pts[0].to_h
D = Data.define(:a, :b)
ds = [D.new(5, 6), D.new(7, 8)]
p ds.min_by { |d| d.a }.to_h
