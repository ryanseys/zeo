# Struct and Data values selected by a member and rendered as hashes.
Point = Struct.new(:x, :y)
pts = [Point.new(1, 2), Point.new(3, 4)]
p pts.min_by { |pt| pt.x }.to_h
p pts.max_by { |pt| pt.x }.to_h
p pts[0].to_h
D = Data.define(:a, :b)
ds = [D.new(5, 6), D.new(7, 8)]
p ds.min_by { |d| d.a }.to_h
__END__
{x: 1, y: 2}
{x: 3, y: 4}
{x: 1, y: 2}
{a: 5, b: 6}
